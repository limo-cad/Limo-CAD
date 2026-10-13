//! Production viewport UI for the native Bevy surface.
//!
//! This module deliberately uses Bevy's stable core UI primitives instead of
//! `bevy_ui_widgets`: the latter is still documented as experimental and
//! unstyled. Keeping the visual tokens and small component
//! builders here gives the production host and the dev capture lab one
//! canonical implementation.

use std::fs;

use bevy::{
    prelude::*,
    text::FontWeight,
    ui::{BoxShadow, UiTransform},
};

use crate::app_preferences::{locale as dictionary, Locale};

use super::{
    ViewportCamera, ViewportConstraintIcon, ViewportHud, ViewportHudSelection, ViewportPalette,
    ViewportToolIcon,
};

#[cfg(test)]
mod font_tests;

pub(crate) const DIAL_CENTER: f32 = 38.0;
const DIAL_AXIS_LENGTH: f32 = 25.0;

/// Per-window appearance. Native controls and viewport graphics read this same
/// palette without storing application preferences in the CAD document.
#[derive(Resource)]
pub(crate) struct Appearance {
    pub palette: ViewportPalette,
    pub theme: ViewportUiTheme,
    pub revision: u64,
}

pub(crate) fn palette(world: &World) -> ViewportPalette {
    world
        .get_resource::<Appearance>()
        .map_or_else(ViewportPalette::default, |a| a.palette)
}

pub(crate) fn theme(world: &World) -> ViewportUiTheme {
    world.get_resource::<Appearance>().map_or_else(
        || ViewportUiTheme::from_palette(&ViewportPalette::default()),
        |appearance| appearance.theme,
    )
}

pub(crate) fn appearance_revision(world: &World) -> u64 {
    world.get_resource::<Appearance>().map_or(0, |a| a.revision)
}

#[derive(Component)]
pub(crate) struct NativeHudRoot;

#[derive(Component, Clone, Copy)]
pub(crate) struct HudAxisMark {
    pub axis: Vec3,
    pub fraction: f32,
    pub radius: f32,
}

#[derive(Component, Clone, Copy)]
pub(crate) struct HudAxisLabel {
    pub axis: Vec3,
}

#[derive(Resource, Clone, Default)]
pub(crate) struct ViewportUiAssets {
    font: Option<Handle<Font>>,
    semibold: Option<Handle<Font>>,
    monospace: Option<Handle<Font>>,
    fallbacks: Vec<Handle<Font>>,
}

/// Prefer a system UI font with broad glyph coverage so native labels match
/// the desktop UI and CAD marks do not render as tofu. Bevy's embedded
/// default remains a safe fallback on unusual installations.
pub(crate) fn load_system_font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    #[cfg(target_os = "macos")]
    let candidates = [
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    ];
    #[cfg(target_os = "windows")]
    let candidates = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\seguisym.ttf",
    ];
    #[cfg(target_os = "linux")]
    let candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
    ];

    let font = candidates
        .into_iter()
        .find_map(|path| fs::read(path).ok())
        .map(|bytes| fonts.add(Font::from_bytes(bytes)));
    #[cfg(target_os = "windows")]
    let semibold = fs::read(r"C:\Windows\Fonts\seguisb.ttf")
        .ok()
        .map(|bytes| fonts.add(Font::from_bytes(bytes)));
    #[cfg(not(target_os = "windows"))]
    let semibold = None;
    // Keep technical symbols ahead of emoji faces. Parley discovers language
    // and emoji fallback fonts on demand without copying entire collections.
    #[cfg(target_os = "windows")]
    let fallback_candidates: &[&[&str]] = &[&[r"C:\Windows\Fonts\seguisym.ttf"]];
    #[cfg(target_os = "macos")]
    let fallback_candidates: &[&[&str]] = &[&["/System/Library/Fonts/Apple Symbols.ttf"]];
    #[cfg(target_os = "linux")]
    let fallback_candidates: &[&[&str]] = &[
        &[
            "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
        ],
        &[
            "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
        ],
    ];
    let fallbacks = fallback_candidates
        .iter()
        .filter_map(|candidates| candidates.iter().find_map(|path| fs::read(path).ok()))
        .map(|bytes| fonts.add(Font::from_bytes(bytes)))
        .collect();
    #[cfg(target_os = "windows")]
    let code_candidates = [
        r"C:\Windows\Fonts\consola.ttf",
        r"C:\Windows\Fonts\cour.ttf",
    ];
    #[cfg(target_os = "macos")]
    let code_candidates = [
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/Monaco.ttf",
    ];
    #[cfg(target_os = "linux")]
    let code_candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf",
    ];
    let monospace = code_candidates
        .into_iter()
        .find_map(|path| fs::read(path).ok())
        .map(|bytes| fonts.add(Font::from_bytes(bytes)));
    commands.insert_resource(ViewportUiAssets {
        font,
        semibold,
        monospace,
        fallbacks,
    });
}

#[derive(Clone, Copy)]
pub struct ViewportUiTheme {
    pub viewport: Color,
    pub panel: Color,
    pub header: Color,
    pub edge: Color,
    pub ink: Color,
    pub mute: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub hover: Color,
    pub shadow: Color,
    pub dialog_shadow: Color,
}

impl ViewportUiTheme {
    pub(crate) fn code_text(self, assets: &ViewportUiAssets, size: f32) -> TextFont {
        let mut text = self.text(assets, size, FontWeight::NORMAL);
        if let Some(font) = &assets.monospace {
            text.font = FontSource::list(
                std::iter::once(FontSource::from(font.clone())).chain(std::iter::once(text.font)),
            );
        }
        text
    }
    pub fn from_palette(palette: &ViewportPalette) -> Self {
        let light = relative_luminance(palette.background) > 0.52;
        Self {
            viewport: rgb(palette.background),
            panel: rgba(palette.panel, 0.96),
            header: rgba(palette.header, 0.96),
            edge: rgba(palette.ui_edge, 0.94),
            ink: rgb(palette.ink),
            mute: rgb(palette.mute),
            accent: rgb(palette.accent),
            accent_soft: rgba(palette.accent, if light { 0.16 } else { 0.30 }),
            hover: rgba(palette.ui_edge, if light { 0.62 } else { 0.78 }),
            shadow: Color::srgba(0.0, 0.0, 0.0, if light { 0.14 } else { 0.28 }),
            dialog_shadow: Color::srgba(0.0, 0.0, 0.0, if light { 0.18 } else { 0.42 }),
        }
    }

    pub(crate) fn text(self, assets: &ViewportUiAssets, size: f32, weight: FontWeight) -> TextFont {
        let mut text = TextFont::from_font_size(size).with_font_weight(weight);
        let face = if weight == FontWeight::SEMIBOLD {
            assets.semibold.as_ref().or(assets.font.as_ref())
        } else {
            assets.font.as_ref()
        };
        if let Some(font) = face {
            text = text.with_font(font.clone());
        }
        if !assets.fallbacks.is_empty() {
            text.font = FontSource::list(
                std::iter::once(text.font)
                    .chain(assets.fallbacks.iter().cloned().map(FontSource::from)),
            );
        }
        text
    }

    fn card_shadow(self) -> BoxShadow {
        BoxShadow::new(self.shadow, px(0.0), px(6.0), px(0.0), px(16.0))
    }

    #[cfg(feature = "dev-ui-lab")]
    fn dialog_shadow(self) -> BoxShadow {
        BoxShadow::new(self.dialog_shadow, px(0.0), px(18.0), px(0.0), px(48.0))
    }
}

fn relative_luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

fn rgb(value: [f32; 3]) -> Color {
    Color::srgb(value[0], value[1], value[2])
}

fn rgba(value: [f32; 3], alpha: f32) -> Color {
    Color::srgba(value[0], value[1], value[2], alpha)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlVisual {
    Idle,
    Hovered,
    Pressed,
    Active,
    Disabled,
}

fn visual_state(hud: &ViewportHud, id: &str, active: bool, disabled: bool) -> ControlVisual {
    if disabled {
        ControlVisual::Disabled
    } else if hud.pressed_control == id {
        ControlVisual::Pressed
    } else if hud.hovered_control == id {
        ControlVisual::Hovered
    } else if active {
        ControlVisual::Active
    } else {
        ControlVisual::Idle
    }
}

fn button_colors(theme: ViewportUiTheme, state: ControlVisual) -> (Color, Color, Color) {
    match state {
        ControlVisual::Idle => (Color::NONE, theme.mute, theme.edge),
        ControlVisual::Hovered => (theme.hover, theme.ink, theme.accent),
        ControlVisual::Pressed => (theme.accent_soft, theme.accent, theme.accent),
        ControlVisual::Active => (theme.accent_soft, theme.accent, theme.accent),
        ControlVisual::Disabled => (
            Color::NONE,
            theme.mute.with_alpha(0.35),
            theme.edge.with_alpha(0.35),
        ),
    }
}

pub(crate) fn spawn_viewport_hud(
    commands: &mut Commands,
    camera: Entity,
    hud: &ViewportHud,
    palette: &ViewportPalette,
    assets: &ViewportUiAssets,
    locale: Locale,
) {
    let theme = ViewportUiTheme::from_palette(palette);

    if hud.dim_opacity > 0.001 {
        commands.spawn((
            Name::new("Native modal dim layer"),
            NativeHudRoot,
            UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgba(
                0.0,
                0.0,
                0.0,
                hud.dim_opacity.clamp(0.0, 0.85),
            )),
            ZIndex(1_000),
        ));
    }
    if let Some(selection) = &hud.selection {
        spawn_selection_hud(
            commands,
            camera,
            selection,
            theme,
            assets,
            if hud.render_native_chrome { 48. } else { 108. },
        );
    }
    if !hud.render_native_chrome {
        return;
    }

    spawn_orientation_dial(commands, camera, hud, palette, theme, assets, locale);
    spawn_navigation_bar(commands, camera, hud, theme, assets);
    if let Some(prompt) = hud.prompt.as_ref().filter(|text| !text.is_empty()) {
        spawn_prompt(commands, camera, prompt, theme, assets);
    }
    if let Some(dof) = hud.dof_label.as_ref().filter(|text| !text.is_empty()) {
        spawn_status_chip(commands, camera, dof, 160.0, theme, assets);
    }
    if let Some(readout) = hud
        .coordinate_readout
        .as_ref()
        .filter(|text| !text.is_empty())
    {
        spawn_status_chip(commands, camera, readout, 12.0, theme, assets);
    }
}

fn spawn_orientation_dial(
    commands: &mut Commands,
    camera: Entity,
    hud: &ViewportHud,
    palette: &ViewportPalette,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
    locale: Locale,
) {
    commands
        .spawn((
            Name::new("Native orientation dial"),
            NativeHudRoot,
            UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                right: px(12.0),
                top: px(12.0),
                width: px(132.0),
                padding: UiRect::axes(px(8.0), px(6.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(12.0)),
                ..default()
            },
            BackgroundColor(theme.panel),
            BorderColor::all(theme.edge),
            theme.card_shadow(),
            ZIndex(20),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(dictionary::translate(locale, "orientationDial.label")),
                theme.text(assets, 8.0, FontWeight::SEMIBOLD),
                TextColor(theme.mute),
                Node {
                    height: px(12.0),
                    ..default()
                },
            ));

            card.spawn(Node {
                position_type: PositionType::Relative,
                width: px(104.0),
                height: px(104.0),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|dial| {
                let orbit_state = visual_state(hud, "orientation:orbit", false, false);
                let (orbit_fill, _, orbit_edge) = button_colors(theme, orbit_state);
                dial.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(14.0),
                        top: px(14.0),
                        width: px(76.0),
                        height: px(76.0),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::MAX,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(if orbit_state == ControlVisual::Idle {
                        rgba(palette.background, 0.82)
                    } else {
                        orbit_fill
                    }),
                    BorderColor::all(if orbit_state == ControlVisual::Idle {
                        theme.edge
                    } else {
                        orbit_edge
                    }),
                ))
                .with_children(|indicator| {
                    for angle_index in 0..20 {
                        let angle = angle_index as f32 * std::f32::consts::TAU / 20.0;
                        indicator.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(37.0 + angle.cos() * 31.0),
                                top: px(37.0 + angle.sin() * 31.0),
                                width: px(2.0),
                                height: px(2.0),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(rgba(palette.grid_major, 0.65)),
                        ));
                    }

                    for (axis, label, color) in [
                        (Vec3::X, "X", Color::srgb(0.88, 0.36, 0.39)),
                        (Vec3::Y, "Y", Color::srgb(0.35, 0.68, 0.45)),
                        (Vec3::Z, "Z", Color::srgb(0.26, 0.65, 0.91)),
                    ] {
                        for index in 1..=9 {
                            let fraction = index as f32 / 9.0;
                            let radius = if index == 9 { 2.5 } else { 1.15 };
                            indicator.spawn((
                                HudAxisMark {
                                    axis,
                                    fraction,
                                    radius,
                                },
                                Node {
                                    position_type: PositionType::Absolute,
                                    width: px(radius * 2.0),
                                    height: px(radius * 2.0),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(color),
                            ));
                        }
                        indicator.spawn((
                            HudAxisLabel { axis },
                            Text::new(label),
                            theme.text(assets, 8.0, FontWeight::EXTRA_BOLD),
                            TextColor(color),
                            Node {
                                position_type: PositionType::Absolute,
                                ..default()
                            },
                        ));
                    }

                    indicator.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(35.5),
                            top: px(35.5),
                            width: px(5.0),
                            height: px(5.0),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.ink),
                    ));
                });

                for (label, preset, left, top) in [
                    ("F", "front", 38.0, 0.0),
                    ("R", "right", 76.0, 42.0),
                    ("B", "back", 38.0, 84.0),
                    ("L", "left", 0.0, 42.0),
                ] {
                    spawn_dial_button(
                        dial,
                        label,
                        &format!("orientation:{preset}"),
                        left,
                        top,
                        hud,
                        theme,
                        assets,
                    );
                }
            });

            card.spawn(Node {
                width: percent(100.0),
                height: px(20.0),
                margin: UiRect::top(px(4.0)),
                column_gap: px(4.0),
                ..default()
            })
            .with_children(|row| {
                for (label, preset, emphasized) in [
                    ("+Z", "top", false),
                    ("ISO", "axonometric", true),
                    ("−Z", "bottom", false),
                ] {
                    let id = format!("orientation:{preset}");
                    let state = visual_state(hud, &id, false, false);
                    let (fill, text, edge) = button_colors(theme, state);
                    row.spawn((
                        Node {
                            height: px(20.0),
                            flex_grow: 1.0,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        BackgroundColor(if state == ControlVisual::Idle {
                            theme.header
                        } else {
                            fill
                        }),
                        BorderColor::all(if state == ControlVisual::Idle {
                            theme.edge
                        } else {
                            edge
                        }),
                    ))
                    .with_child((
                        Text::new(label),
                        theme.text(
                            assets,
                            9.0,
                            if emphasized {
                                FontWeight::BOLD
                            } else {
                                FontWeight::SEMIBOLD
                            },
                        ),
                        TextColor(if state == ControlVisual::Idle {
                            if emphasized {
                                theme.ink
                            } else {
                                theme.mute
                            }
                        } else {
                            text
                        }),
                    ));
                }
            });
            card.spawn((
                Text::new(dictionary::translate(locale, "orientationDial.orbit")),
                theme.text(assets, 8.0, FontWeight::NORMAL),
                TextColor(theme.mute.with_alpha(0.72)),
                Node {
                    margin: UiRect::top(px(3.0)),
                    ..default()
                },
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_dial_button(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    label: &'static str,
    id: &str,
    left: f32,
    top: f32,
    hud: &ViewportHud,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) {
    let state = visual_state(hud, id, false, false);
    let (fill, text, edge) = button_colors(theme, state);
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(28.0),
                height: px(20.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(10.0)),
                ..default()
            },
            BackgroundColor(if state == ControlVisual::Idle {
                theme.header
            } else {
                fill
            }),
            BorderColor::all(if state == ControlVisual::Idle {
                theme.edge
            } else {
                edge
            }),
            ZIndex(2),
        ))
        .with_child((
            Text::new(label),
            theme.text(assets, 9.0, FontWeight::BOLD),
            TextColor(if state == ControlVisual::Idle {
                theme.mute
            } else {
                text
            }),
        ));
}

#[derive(Clone, Copy)]
enum Icon {
    Undo,
    Redo,
    Focus,
    Orbit,
    Pan,
    Zoom,
    ZoomWindow,
    Fit,
    Display,
    Grid,
    Gamepad,
}

struct NavButton {
    id: &'static str,
    icon: Icon,
    active: bool,
    disabled: bool,
}

fn spawn_navigation_bar(
    commands: &mut Commands,
    camera: Entity,
    hud: &ViewportHud,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) {
    commands
        .spawn((
            Name::new("Native viewport navigation"),
            NativeHudRoot,
            UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                bottom: px(12.0),
                width: percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            ZIndex(20),
        ))
        .with_children(|center| {
            center
                .spawn((
                    Node {
                        height: px(34.0),
                        padding: UiRect::axes(px(6.0), px(4.0)),
                        align_items: AlignItems::Center,
                        column_gap: px(2.0),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BackgroundColor(theme.header),
                    BorderColor::all(theme.edge),
                    theme.card_shadow(),
                ))
                .with_children(|bar| {
                    if hud.sketch_mode {
                        for button in [
                            NavButton {
                                id: "undo",
                                icon: Icon::Undo,
                                active: false,
                                disabled: !hud.can_undo,
                            },
                            NavButton {
                                id: "redo",
                                icon: Icon::Redo,
                                active: false,
                                disabled: !hud.can_redo,
                            },
                            NavButton {
                                id: "lookAtSketch",
                                icon: Icon::Focus,
                                active: false,
                                disabled: false,
                            },
                        ] {
                            spawn_nav_button(bar, button, hud, theme);
                        }
                        spawn_separator(bar, theme.edge);
                    }

                    for button in [
                        NavButton {
                            id: "orbit",
                            icon: Icon::Orbit,
                            active: hud.nav_tool == "orbit",
                            disabled: false,
                        },
                        NavButton {
                            id: "pan",
                            icon: Icon::Pan,
                            active: hud.nav_tool == "pan",
                            disabled: false,
                        },
                        NavButton {
                            id: "zoom",
                            icon: Icon::Zoom,
                            active: hud.nav_tool == "zoom",
                            disabled: false,
                        },
                        NavButton {
                            id: "zoomWindow",
                            icon: Icon::ZoomWindow,
                            active: hud.nav_tool == "zoomWindow",
                            disabled: false,
                        },
                        NavButton {
                            id: "fit",
                            icon: Icon::Fit,
                            active: false,
                            disabled: false,
                        },
                        NavButton {
                            id: "displaySettings",
                            icon: Icon::Display,
                            active: false,
                            disabled: true,
                        },
                        NavButton {
                            id: "gridSettings",
                            icon: Icon::Grid,
                            active: false,
                            disabled: true,
                        },
                    ] {
                        spawn_nav_button(bar, button, hud, theme);
                    }
                    spawn_separator(bar, theme.edge);
                    spawn_nav_button(
                        bar,
                        NavButton {
                            id: "sixDof",
                            icon: Icon::Gamepad,
                            active: hud.six_dof_state == "connected",
                            disabled: hud.six_dof_state == "unsupported",
                        },
                        hud,
                        theme,
                    );

                    let indicator = match hud.six_dof_state.as_str() {
                        "connected" => Color::srgb(0.29, 0.82, 0.52),
                        "connecting" => Color::srgb(0.98, 0.73, 0.24),
                        "error" => Color::srgb(0.97, 0.27, 0.34),
                        _ => theme.mute,
                    };
                    bar.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(8.0),
                            bottom: px(6.0),
                            width: px(6.0),
                            height: px(6.0),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(indicator),
                        BorderColor::all(theme.header),
                    ));
                });
        });

    let _ = assets;
}

fn spawn_nav_button(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    button: NavButton,
    hud: &ViewportHud,
    theme: ViewportUiTheme,
) {
    let id = format!("nav:{}", button.id);
    let state = visual_state(hud, &id, button.active, button.disabled);
    let (fill, icon, _) = button_colors(theme, state);
    parent
        .spawn((
            Node {
                width: px(24.0),
                height: px(24.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(fill),
        ))
        .with_children(|root| spawn_icon(root, button.icon, icon));
}

fn spawn_separator(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands, color: Color) {
    parent.spawn((
        Node {
            width: px(1.0),
            height: px(16.0),
            margin: UiRect::horizontal(px(4.0)),
            ..default()
        },
        BackgroundColor(color),
    ));
}

fn spawn_icon(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands, icon: Icon, color: Color) {
    parent
        .spawn(Node {
            position_type: PositionType::Relative,
            width: px(16.0),
            height: px(16.0),
            ..default()
        })
        .with_children(|canvas| {
            let segments: &[(f32, f32, f32, f32)] = match icon {
                Icon::Undo => &[
                    (7.0, 4.0, 3.0, 7.0),
                    (3.0, 7.0, 7.0, 10.0),
                    (3.0, 7.0, 10.0, 7.0),
                    (10.0, 7.0, 13.0, 10.0),
                ],
                Icon::Redo => &[
                    (9.0, 4.0, 13.0, 7.0),
                    (13.0, 7.0, 9.0, 10.0),
                    (3.0, 7.0, 13.0, 7.0),
                    (3.0, 7.0, 6.0, 10.0),
                ],
                Icon::Focus => &[
                    (2.0, 6.0, 2.0, 2.0),
                    (2.0, 2.0, 6.0, 2.0),
                    (10.0, 2.0, 14.0, 2.0),
                    (14.0, 2.0, 14.0, 6.0),
                    (14.0, 10.0, 14.0, 14.0),
                    (14.0, 14.0, 10.0, 14.0),
                    (6.0, 14.0, 2.0, 14.0),
                    (2.0, 14.0, 2.0, 10.0),
                ],
                Icon::Orbit => &[
                    (8.0, 2.0, 8.0, 14.0),
                    (2.0, 8.0, 14.0, 8.0),
                    (8.0, 2.0, 6.0, 4.0),
                    (8.0, 2.0, 10.0, 4.0),
                    (14.0, 8.0, 12.0, 6.0),
                    (14.0, 8.0, 12.0, 10.0),
                ],
                Icon::Pan => &[
                    (5.0, 7.0, 5.0, 3.0),
                    (7.5, 7.0, 7.5, 2.0),
                    (10.0, 7.0, 10.0, 3.0),
                    (12.5, 8.0, 12.5, 5.0),
                    (5.0, 7.0, 3.0, 7.0),
                    (3.0, 7.0, 5.0, 13.0),
                    (5.0, 13.0, 11.0, 13.0),
                    (11.0, 13.0, 12.5, 8.0),
                ],
                Icon::Zoom => &[
                    (11.0, 11.0, 15.0, 15.0),
                    (4.0, 8.0, 10.0, 8.0),
                    (7.0, 5.0, 7.0, 11.0),
                ],
                Icon::ZoomWindow => &[
                    (2.0, 6.0, 2.0, 2.0),
                    (2.0, 2.0, 6.0, 2.0),
                    (10.0, 2.0, 14.0, 2.0),
                    (14.0, 2.0, 14.0, 6.0),
                    (14.0, 10.0, 14.0, 14.0),
                    (14.0, 14.0, 10.0, 14.0),
                    (6.0, 14.0, 2.0, 14.0),
                    (2.0, 14.0, 2.0, 10.0),
                ],
                Icon::Fit => &[
                    (2.0, 6.0, 2.0, 2.0),
                    (2.0, 2.0, 6.0, 2.0),
                    (10.0, 2.0, 14.0, 2.0),
                    (14.0, 2.0, 14.0, 6.0),
                    (14.0, 10.0, 14.0, 14.0),
                    (14.0, 14.0, 10.0, 14.0),
                    (6.0, 14.0, 2.0, 14.0),
                    (2.0, 14.0, 2.0, 10.0),
                ],
                Icon::Display => &[
                    (2.0, 3.0, 14.0, 3.0),
                    (14.0, 3.0, 14.0, 11.0),
                    (14.0, 11.0, 2.0, 11.0),
                    (2.0, 11.0, 2.0, 3.0),
                    (8.0, 11.0, 8.0, 14.0),
                    (5.0, 14.0, 11.0, 14.0),
                ],
                Icon::Grid => &[
                    (3.0, 2.0, 3.0, 14.0),
                    (8.0, 2.0, 8.0, 14.0),
                    (13.0, 2.0, 13.0, 14.0),
                    (2.0, 3.0, 14.0, 3.0),
                    (2.0, 8.0, 14.0, 8.0),
                    (2.0, 13.0, 14.0, 13.0),
                ],
                Icon::Gamepad => &[
                    (3.0, 6.0, 5.0, 3.0),
                    (5.0, 3.0, 11.0, 3.0),
                    (11.0, 3.0, 13.0, 6.0),
                    (13.0, 6.0, 14.0, 12.0),
                    (14.0, 12.0, 11.0, 13.0),
                    (11.0, 13.0, 9.0, 10.0),
                    (9.0, 10.0, 7.0, 10.0),
                    (7.0, 10.0, 5.0, 13.0),
                    (5.0, 13.0, 2.0, 12.0),
                    (2.0, 12.0, 3.0, 6.0),
                    (4.0, 8.0, 8.0, 8.0),
                    (6.0, 6.0, 6.0, 10.0),
                ],
            };
            for &(x1, y1, x2, y2) in segments {
                spawn_segment(canvas, x1, y1, x2, y2, color);
            }
            if matches!(icon, Icon::Zoom) {
                canvas.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(1.5),
                        top: px(2.5),
                        width: px(11.0),
                        height: px(11.0),
                        border: UiRect::all(px(1.4)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BorderColor::all(color),
                ));
            }
            if matches!(icon, Icon::Gamepad) {
                for (left, top) in [(10.0, 6.0), (12.0, 8.0)] {
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(left),
                            top: px(top),
                            width: px(2.0),
                            height: px(2.0),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                }
            }
        });
}

fn spawn_segment(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: Color,
) {
    let delta = Vec2::new(x2 - x1, y2 - y1);
    let length = delta.length().max(0.5);
    let midpoint = Vec2::new((x1 + x2) * 0.5, (y1 + y2) * 0.5);
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(midpoint.x - length * 0.5),
            top: px(midpoint.y - 0.7),
            width: px(length),
            height: px(1.4),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        UiTransform::from_rotation(Rot2::radians(delta.y.atan2(delta.x))),
        BackgroundColor(color),
    ));
}

fn spawn_scaled_segment(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    segment: (f32, f32, f32, f32),
    scale: f32,
    color: Color,
) {
    let (x1, y1, x2, y2) = segment;
    let delta = Vec2::new((x2 - x1) * scale, (y2 - y1) * scale);
    let length = delta.length().max(0.5);
    let midpoint = Vec2::new((x1 + x2) * 0.5 * scale, (y1 + y2) * 0.5 * scale);
    let thickness = (1.55 * scale).clamp(1.15, 1.8);
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(midpoint.x - length * 0.5),
            top: px(midpoint.y - thickness * 0.5),
            width: px(length),
            height: px(thickness),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        UiTransform::from_rotation(Rot2::radians(delta.y.atan2(delta.x))),
        BackgroundColor(color),
    ));
}

fn spawn_icon_ring(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    left: f32,
    top: f32,
    diameter: f32,
    scale: f32,
    color: Color,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(left * scale),
            top: px(top * scale),
            width: px(diameter * scale),
            height: px(diameter * scale),
            border: UiRect::all(px((1.45 * scale).clamp(1.0, 1.7))),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BorderColor::all(color),
    ));
}

fn spawn_icon_dot(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    center_x: f32,
    center_y: f32,
    diameter: f32,
    scale: f32,
    color: Color,
) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px((center_x - diameter * 0.5) * scale),
            top: px((center_y - diameter * 0.5) * scale),
            width: px(diameter * scale),
            height: px(diameter * scale),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(color),
    ));
}

fn spawn_icon_rect(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    rect: (f32, f32, f32, f32),
    radius: f32,
    scale: f32,
    color: Color,
) {
    let (left, top, width, height) = rect;
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(left * scale),
            top: px(top * scale),
            width: px(width * scale),
            height: px(height * scale),
            border: UiRect::all(px((1.45 * scale).clamp(1.0, 1.7))),
            border_radius: BorderRadius::all(px(radius * scale)),
            ..default()
        },
        BorderColor::all(color),
    ));
}

/// Constraint icons use procedural strokes on a 24×24 canvas, independent of
/// raster assets and installed fonts. The exhaustive match covers every icon.
pub(crate) fn spawn_constraint_icon(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    icon: ViewportConstraintIcon,
    color: Color,
    size: f32,
) {
    let scale = size / 24.0;
    parent
        .spawn(Node {
            position_type: PositionType::Relative,
            width: px(size),
            height: px(size),
            ..default()
        })
        .with_children(|canvas| {
            let segments: &[(f32, f32, f32, f32)] = match icon {
                ViewportConstraintIcon::HorizontalVertical => &[
                    (4.0, 7.0, 20.0, 7.0),
                    (17.0, 4.0, 17.0, 20.0),
                    (13.0, 7.0, 17.0, 7.0),
                    (17.0, 7.0, 17.0, 11.0),
                ],
                ViewportConstraintIcon::Horizontal => &[(4.0, 12.0, 20.0, 12.0)],
                ViewportConstraintIcon::Vertical => &[(12.0, 4.0, 12.0, 20.0)],
                ViewportConstraintIcon::HorizontalPoints => &[(5.0, 12.0, 19.0, 12.0)],
                ViewportConstraintIcon::VerticalPoints => &[(12.0, 5.0, 12.0, 19.0)],
                ViewportConstraintIcon::Coincident => &[
                    (4.0, 18.0, 12.0, 10.0),
                    (20.0, 18.0, 12.0, 10.0),
                    (12.0, 3.0, 12.0, 7.0),
                    (5.0, 10.0, 9.0, 10.0),
                    (15.0, 10.0, 19.0, 10.0),
                ],
                ViewportConstraintIcon::Tangent => &[(4.0, 7.0, 20.0, 12.0)],
                ViewportConstraintIcon::Equal => &[(5.0, 9.0, 19.0, 9.0), (5.0, 15.0, 19.0, 15.0)],
                ViewportConstraintIcon::Parallel => &[
                    (5.0, 19.0, 10.0, 5.0),
                    (14.0, 19.0, 19.0, 5.0),
                    (7.0, 10.0, 10.0, 9.0),
                    (14.0, 15.0, 17.0, 14.0),
                ],
                ViewportConstraintIcon::Perpendicular => &[
                    (5.0, 4.0, 5.0, 19.0),
                    (5.0, 19.0, 20.0, 19.0),
                    (5.0, 14.0, 10.0, 14.0),
                    (10.0, 14.0, 10.0, 19.0),
                ],
                ViewportConstraintIcon::Fix => &[
                    (7.0, 11.0, 7.0, 8.0),
                    (7.0, 8.0, 9.0, 4.5),
                    (9.0, 4.5, 15.0, 4.5),
                    (15.0, 4.5, 17.0, 8.0),
                    (17.0, 8.0, 17.0, 11.0),
                ],
                ViewportConstraintIcon::Midpoint => &[
                    (3.0, 18.0, 21.0, 18.0),
                    (12.0, 6.0, 8.0, 13.0),
                    (8.0, 13.0, 16.0, 13.0),
                    (16.0, 13.0, 12.0, 6.0),
                    (12.0, 13.0, 12.0, 18.0),
                ],
                ViewportConstraintIcon::Concentric => &[],

                ViewportConstraintIcon::ArcEndpoint => &[
                    (4.0, 18.0, 4.78, 14.72),
                    (4.78, 14.72, 6.61, 11.89),
                    (6.61, 11.89, 9.27, 9.83),
                    (9.27, 9.83, 12.47, 8.76),
                    (12.47, 8.76, 15.84, 8.82),
                    (15.84, 8.82, 18.99, 10.0),
                ],
                ViewportConstraintIcon::Collinear => &[
                    (3.0, 18.0, 9.0, 14.0),
                    (10.5, 13.0, 15.0, 10.0),
                    (16.5, 9.0, 21.0, 6.0),
                    (4.0, 15.0, 10.0, 11.0),
                    (14.0, 9.0, 20.0, 5.0),
                ],
                ViewportConstraintIcon::Symmetry => &[
                    (12.0, 3.0, 12.0, 8.0),
                    (12.0, 10.0, 12.0, 14.0),
                    (12.0, 16.0, 12.0, 21.0),
                    (4.0, 7.0, 9.0, 12.0),
                    (9.0, 12.0, 4.0, 17.0),
                    (20.0, 7.0, 15.0, 12.0),
                    (15.0, 12.0, 20.0, 17.0),
                    (7.0, 12.0, 17.0, 12.0),
                ],
            };
            for &segment in segments {
                spawn_scaled_segment(canvas, segment, scale, color);
            }
            match icon {
                ViewportConstraintIcon::HorizontalPoints => {
                    spawn_icon_dot(canvas, 5.0, 12.0, 3.6, scale, color);
                    spawn_icon_dot(canvas, 19.0, 12.0, 3.6, scale, color);
                }
                ViewportConstraintIcon::VerticalPoints => {
                    spawn_icon_dot(canvas, 12.0, 5.0, 3.6, scale, color);
                    spawn_icon_dot(canvas, 12.0, 19.0, 3.6, scale, color);
                }
                ViewportConstraintIcon::Coincident => {
                    spawn_icon_ring(canvas, 9.8, 7.8, 4.4, scale, color);
                }
                ViewportConstraintIcon::Tangent => {
                    spawn_icon_ring(canvas, 5.0, 8.0, 12.0, scale, color);
                    spawn_icon_dot(canvas, 11.0, 9.2, 2.4, scale, color);
                }
                ViewportConstraintIcon::Fix => {
                    spawn_icon_rect(canvas, (5.0, 11.0, 14.0, 10.0), 2.0, scale, color);
                    spawn_icon_dot(canvas, 12.0, 16.0, 2.6, scale, color);
                }
                ViewportConstraintIcon::Concentric => {
                    spawn_icon_ring(canvas, 4.0, 4.0, 16.0, scale, color);
                    spawn_icon_ring(canvas, 8.0, 8.0, 8.0, scale, color);
                    spawn_icon_dot(canvas, 12.0, 12.0, 2.0, scale, color);
                }
                ViewportConstraintIcon::Collinear => {
                    spawn_icon_dot(canvas, 12.0, 10.0, 2.4, scale, color);
                }
                ViewportConstraintIcon::ArcEndpoint => {
                    spawn_icon_dot(canvas, 19.0, 10.0, 2.4, scale, color);
                }
                _ => {}
            }
        });
}

/// Compact active sketch-command symbol used beside the pointer. The shapes
/// intentionally follow the corresponding toolbar diagrams at a quieter size.
pub(crate) fn spawn_tool_icon(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    icon: ViewportToolIcon,
    color: Color,
    size: f32,
) {
    let scale = size / 16.0;
    parent
        .spawn(Node {
            position_type: PositionType::Relative,
            width: px(size),
            height: px(size),
            ..default()
        })
        .with_children(|canvas| {
            let segments: &[(f32, f32, f32, f32)] = match icon {
                ViewportToolIcon::Line | ViewportToolIcon::MidpointLine => {
                    &[(2.0, 13.0, 14.0, 3.0)]
                }
                ViewportToolIcon::Point => &[(3.0, 8.0, 13.0, 8.0), (8.0, 3.0, 8.0, 13.0)],
                ViewportToolIcon::Rectangle => &[
                    (2.0, 3.0, 14.0, 3.0),
                    (14.0, 3.0, 14.0, 13.0),
                    (14.0, 13.0, 2.0, 13.0),
                    (2.0, 13.0, 2.0, 3.0),
                ],
                ViewportToolIcon::Circle => &[],
                ViewportToolIcon::Arc => &[
                    (3.0, 12.0, 5.0, 7.0),
                    (5.0, 7.0, 9.0, 4.0),
                    (9.0, 4.0, 14.0, 5.0),
                ],
                ViewportToolIcon::Dimension => &[
                    (2.0, 4.0, 2.0, 12.0),
                    (14.0, 4.0, 14.0, 12.0),
                    (2.0, 8.0, 14.0, 8.0),
                    (2.0, 8.0, 5.0, 6.0),
                    (2.0, 8.0, 5.0, 10.0),
                    (14.0, 8.0, 11.0, 6.0),
                    (14.0, 8.0, 11.0, 10.0),
                ],
                ViewportToolIcon::Fillet => &[
                    (2.0, 13.0, 2.0, 7.0),
                    (2.0, 7.0, 5.0, 4.0),
                    (5.0, 4.0, 13.0, 4.0),
                ],
                ViewportToolIcon::Chamfer => &[
                    (2.0, 13.0, 2.0, 8.0),
                    (2.0, 8.0, 6.0, 4.0),
                    (6.0, 4.0, 14.0, 4.0),
                ],
                ViewportToolIcon::Offset => &[
                    (2.0, 13.0, 2.0, 5.0),
                    (2.0, 5.0, 10.0, 5.0),
                    (6.0, 15.0, 6.0, 9.0),
                    (6.0, 9.0, 14.0, 9.0),
                ],
                ViewportToolIcon::Trim => &[
                    (3.0, 3.0, 13.0, 13.0),
                    (13.0, 3.0, 3.0, 13.0),
                    (6.0, 8.0, 10.0, 8.0),
                ],
                ViewportToolIcon::Extend => &[
                    (2.0, 14.0, 8.0, 8.0),
                    (8.0, 8.0, 14.0, 2.0),
                    (11.0, 2.0, 14.0, 2.0),
                    (14.0, 2.0, 14.0, 5.0),
                ],
                ViewportToolIcon::Break => &[
                    (2.0, 14.0, 7.0, 9.0),
                    (9.0, 7.0, 14.0, 2.0),
                    (6.0, 6.0, 9.0, 8.0),
                    (10.0, 10.0, 7.0, 8.0),
                ],
                ViewportToolIcon::Mirror => &[
                    (8.0, 1.0, 8.0, 15.0),
                    (2.0, 12.0, 6.0, 8.0),
                    (6.0, 8.0, 2.0, 4.0),
                    (14.0, 12.0, 10.0, 8.0),
                    (10.0, 8.0, 14.0, 4.0),
                ],
                ViewportToolIcon::MoveCopy => &[
                    (8.0, 1.0, 8.0, 15.0),
                    (1.0, 8.0, 15.0, 8.0),
                    (8.0, 1.0, 6.0, 4.0),
                    (8.0, 1.0, 10.0, 4.0),
                    (15.0, 8.0, 12.0, 6.0),
                    (15.0, 8.0, 12.0, 10.0),
                ],
                ViewportToolIcon::Scale => &[
                    (3.0, 13.0, 13.0, 3.0),
                    (13.0, 3.0, 9.0, 3.0),
                    (13.0, 3.0, 13.0, 7.0),
                    (3.0, 13.0, 6.0, 13.0),
                    (3.0, 13.0, 3.0, 10.0),
                ],
                ViewportToolIcon::Polygon => &[
                    (8.0, 1.5, 13.5, 5.0),
                    (13.5, 5.0, 12.0, 12.5),
                    (12.0, 12.5, 4.0, 12.5),
                    (4.0, 12.5, 2.5, 5.0),
                    (2.5, 5.0, 8.0, 1.5),
                ],
                ViewportToolIcon::Slot => &[],
                ViewportToolIcon::Spline => &[
                    (2.0, 12.0, 5.0, 6.0),
                    (5.0, 6.0, 9.0, 10.0),
                    (9.0, 10.0, 14.0, 3.0),
                ],
            };
            for &segment in segments {
                spawn_scaled_segment(canvas, segment, scale, color);
            }
            match icon {
                ViewportToolIcon::Line => {
                    spawn_icon_dot(canvas, 2.0, 13.0, 2.4, scale, color);
                    spawn_icon_dot(canvas, 14.0, 3.0, 2.4, scale, color);
                }
                ViewportToolIcon::MidpointLine => {
                    spawn_icon_dot(canvas, 2.0, 13.0, 2.2, scale, color);
                    spawn_icon_dot(canvas, 14.0, 3.0, 2.2, scale, color);
                    spawn_icon_dot(canvas, 8.0, 8.0, 2.6, scale, color);
                }
                ViewportToolIcon::Point => {
                    spawn_icon_ring(canvas, 5.5, 5.5, 5.0, scale, color);
                }
                ViewportToolIcon::Circle => {
                    spawn_icon_ring(canvas, 2.0, 2.0, 12.0, scale, color);
                    spawn_icon_dot(canvas, 8.0, 8.0, 2.0, scale, color);
                }
                ViewportToolIcon::Slot => {
                    spawn_icon_rect(canvas, (2.0, 5.0, 12.0, 6.0), 3.0, scale, color);
                }
                _ => {}
            }
        });
}

fn spawn_selection_hud(
    commands: &mut Commands,
    camera: Entity,
    selection: &ViewportHudSelection,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
    bottom: f32,
) {
    commands
        .spawn((
            Name::new("Native selection readout"),
            NativeHudRoot,
            UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                right: px(12.0),
                bottom: px(bottom),
                min_width: px(208.0),
                max_width: px(280.0),
                padding: UiRect::all(px(10.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BackgroundColor(theme.header),
            BorderColor::all(theme.edge),
            theme.card_shadow(),
            ZIndex(20),
        ))
        .with_children(|card| {
            card.spawn(Node {
                min_width: px(218.0),
                padding: UiRect::bottom(px(6.0)),
                margin: UiRect::bottom(px(2.0)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: px(16.0),
                border: UiRect::bottom(px(1.0)),
                ..default()
            })
            .with_children(|header_row| {
                header_row.spawn((
                    Text::new(selection.title.clone()),
                    theme.text(assets, 9.0, FontWeight::SEMIBOLD),
                    TextColor(theme.mute),
                ));
                header_row.spawn((
                    Text::new(selection.subject.clone()),
                    theme.text(assets, 11.0, FontWeight::MEDIUM),
                    TextColor(theme.ink),
                ));
            });

            for row in &selection.rows {
                card.spawn(Node {
                    width: percent(100.0),
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(16.0),
                    ..default()
                })
                .with_children(|values| {
                    values.spawn((
                        Text::new(row.label.clone()),
                        theme.text(assets, 11.0, FontWeight::NORMAL),
                        TextColor(theme.mute),
                    ));
                    values.spawn((
                        Text::new(row.value.clone()),
                        theme.text(assets, 11.0, FontWeight::MEDIUM),
                        TextColor(theme.ink),
                    ));
                });
            }

            if let Some(footer) = &selection.footer {
                card.spawn((
                    Node {
                        width: percent(100.0),
                        justify_content: JustifyContent::FlexEnd,
                        padding: UiRect::top(px(4.0)),
                        margin: UiRect::top(px(2.0)),
                        border: UiRect::top(px(1.0)),
                        ..default()
                    },
                    BorderColor::all(theme.edge),
                ))
                .with_child((
                    Text::new(footer.clone()),
                    theme.text(assets, 9.0, FontWeight::NORMAL),
                    TextColor(theme.mute),
                ));
            }
        });
}

fn spawn_prompt(
    commands: &mut Commands,
    camera: Entity,
    prompt: &str,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) {
    commands
        .spawn((
            Name::new("Native viewport prompt"),
            NativeHudRoot,
            UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(12.0),
                width: percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            ZIndex(20),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    padding: UiRect::axes(px(12.0), px(6.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                BackgroundColor(theme.header),
                BorderColor::all(theme.edge),
                theme.card_shadow(),
            ))
            .with_child((
                Text::new(prompt.to_owned()),
                theme.text(assets, 12.0, FontWeight::NORMAL),
                TextColor(theme.ink),
            ));
        });
}

fn spawn_status_chip(
    commands: &mut Commands,
    camera: Entity,
    value: &str,
    right: f32,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) {
    commands.spawn((
        Name::new("Native viewport status chip"),
        NativeHudRoot,
        UiTargetCamera(camera),
        Node {
            position_type: PositionType::Absolute,
            right: px(right),
            bottom: px(12.0),
            padding: UiRect::axes(px(8.0), px(4.0)),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(4.0)),
            ..default()
        },
        BackgroundColor(theme.header),
        BorderColor::all(theme.edge),
        Text::new(value.to_owned()),
        theme.text(assets, 10.0, FontWeight::NORMAL),
        TextColor(theme.mute),
        ZIndex(20),
    ));
}

pub(crate) fn update_orientation_nodes(
    camera: ViewportCamera,
    marks: &mut Query<(&HudAxisMark, &mut Node)>,
    labels: &mut Query<(&HudAxisLabel, &mut Node), Without<HudAxisMark>>,
) {
    let position = Vec3::from_array(camera.position);
    let target = Vec3::from_array(camera.target);
    let forward = (target - position).normalize_or_zero();
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let mut right = forward.cross(up_hint).normalize_or_zero();
    if right == Vec3::ZERO {
        right = Vec3::X;
    }
    let screen_up = right.cross(forward).normalize_or_zero();

    for (mark, mut node) in marks {
        let endpoint =
            Vec2::new(mark.axis.dot(right), -mark.axis.dot(screen_up)) * DIAL_AXIS_LENGTH;
        let point = Vec2::splat(DIAL_CENTER) + endpoint * mark.fraction;
        let left = px(point.x - mark.radius);
        let top = px(point.y - mark.radius);
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
    for (label, mut node) in labels {
        let endpoint =
            Vec2::new(label.axis.dot(right), -label.axis.dot(screen_up)) * DIAL_AXIS_LENGTH;
        let point = Vec2::splat(DIAL_CENTER) + endpoint;
        let left = px(point.x + if endpoint.x >= 0.0 { 4.0 } else { -8.0 });
        let top = px(point.y + if endpoint.y >= 0.0 { 2.0 } else { -10.0 });
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
}

/// The dialog sample is not used as a command form in production. It is the
/// visual contract used by the native capture lab for dialog layout and themes.
#[cfg(feature = "dev-ui-lab")]
pub(crate) fn spawn_reference_dialog(
    commands: &mut Commands,
    camera: Entity,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
    locale: Locale,
) {
    commands
        .spawn((
            Name::new("Native feature dialog reference"),
            UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                left: percent(50.0),
                top: percent(50.0),
                width: px(440.0),
                padding: UiRect::all(px(2.0)),
                border_radius: BorderRadius::all(px(14.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            UiTransform::from_translation(Val2::percent(-50.0, -50.0)),
            BackgroundColor(theme.accent.with_alpha(0.78)),
            theme.dialog_shadow(),
            ZIndex(1_100),
        ))
        .with_children(|frame| {
            frame
                .spawn((
                    Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        border_radius: BorderRadius::all(px(12.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                ))
                .with_children(|dialog| {
                    dialog
                        .spawn((
                            Node {
                                height: px(52.0),
                                padding: UiRect::horizontal(px(16.0)),
                                align_items: AlignItems::Center,
                                column_gap: px(10.0),
                                border: UiRect::bottom(px(1.0)),
                                border_radius: BorderRadius::px(12.0, 12.0, 0.0, 0.0),
                                ..default()
                            },
                            BackgroundColor(theme.header),
                            BorderColor::all(theme.edge),
                        ))
                        .with_children(|header| {
                            header.spawn((
                                Node {
                                    width: px(18.0),
                                    height: px(18.0),
                                    border: UiRect::all(px(2.0)),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BorderColor::all(theme.accent),
                            ));
                            header.spawn((
                                Text::new(dictionary::translate(locale, "sketchOrigin.title")),
                                theme.text(assets, 16.0, FontWeight::SEMIBOLD),
                                TextColor(theme.ink),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            header.spawn((
                                Text::new("×"),
                                theme.text(assets, 22.0, FontWeight::NORMAL),
                                TextColor(theme.mute),
                            ));
                        });

                    dialog
                        .spawn(Node {
                            padding: UiRect::all(px(20.0)),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(14.0),
                            ..default()
                        })
                        .with_children(|body| {
                            body.spawn((
                                Text::new(
                                    dictionary::translate(locale, "sketchOrigin.description")
                                        .replace("{face}", "603509456585486"),
                                ),
                                theme.text(assets, 14.0, FontWeight::NORMAL),
                                TextColor(theme.mute),
                                Node {
                                    max_width: px(390.0),
                                    ..default()
                                },
                            ));
                            spawn_dialog_choice(
                                body,
                                true,
                                dictionary::translate(locale, "sketchOrigin.faceCenter"),
                                dictionary::translate(locale, "sketchOrigin.faceCenterHint"),
                                theme,
                                assets,
                            );
                            spawn_dialog_choice(
                                body,
                                false,
                                dictionary::translate(locale, "sketchOrigin.globalProjection"),
                                dictionary::translate(locale, "sketchOrigin.globalProjectionHint"),
                                theme,
                                assets,
                            );
                        });

                    dialog
                        .spawn((
                            Node {
                                height: px(54.0),
                                padding: UiRect::horizontal(px(16.0)),
                                justify_content: JustifyContent::FlexEnd,
                                align_items: AlignItems::Center,
                                column_gap: px(10.0),
                                border: UiRect::top(px(1.0)),
                                border_radius: BorderRadius::px(0.0, 0.0, 12.0, 12.0),
                                ..default()
                            },
                            BackgroundColor(theme.header),
                            BorderColor::all(theme.edge),
                        ))
                        .with_children(|footer| {
                            spawn_dialog_action(
                                footer,
                                dictionary::translate(locale, "sketchOrigin.cancel"),
                                false,
                                theme,
                                assets,
                            );
                            spawn_dialog_action(
                                footer,
                                dictionary::translate(locale, "sketchOrigin.ok"),
                                true,
                                theme,
                                assets,
                            );
                        });
                });
        });
}

#[cfg(feature = "dev-ui-lab")]
fn spawn_dialog_choice(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    selected: bool,
    title: &str,
    hint: &str,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) {
    parent
        .spawn((
            Node {
                min_height: px(74.0),
                padding: UiRect::all(px(12.0)),
                column_gap: px(12.0),
                border: UiRect::all(px(if selected { 1.5 } else { 1.0 })),
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            BackgroundColor(theme.header),
            BorderColor::all(if selected { theme.accent } else { theme.edge }),
        ))
        .with_children(|choice| {
            choice.spawn((
                Node {
                    margin: UiRect::top(px(3.0)),
                    width: px(14.0),
                    height: px(14.0),
                    border: UiRect::all(px(2.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(if selected { theme.accent } else { theme.mute }),
                BackgroundColor(if selected { theme.accent } else { Color::NONE }),
            ));
            choice
                .spawn(Node {
                    flex_grow: 1.0,
                    max_width: px(300.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        Text::new(title.to_owned()),
                        theme.text(assets, 15.0, FontWeight::MEDIUM),
                        TextColor(theme.ink),
                    ));
                    copy.spawn((
                        Text::new(hint.to_owned()),
                        theme.text(assets, 12.0, FontWeight::NORMAL),
                        TextColor(theme.mute),
                    ));
                });
        });
}

#[cfg(feature = "dev-ui-lab")]
fn spawn_dialog_action(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    label: &str,
    primary: bool,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) {
    parent
        .spawn((
            Node {
                height: px(32.0),
                padding: UiRect::horizontal(px(16.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BackgroundColor(if primary { theme.accent } else { Color::NONE }),
            BorderColor::all(if primary { theme.accent } else { theme.edge }),
        ))
        .with_child((
            Text::new(label.to_owned()),
            theme.text(assets, 14.0, FontWeight::SEMIBOLD),
            TextColor(if primary { Color::WHITE } else { theme.ink }),
        ));
}

#[cfg(feature = "dev-ui-lab")]
pub(crate) fn light_reference_palette() -> ViewportPalette {
    ViewportPalette {
        background: [220.0 / 255.0, 227.0 / 255.0, 234.0 / 255.0],
        panel: [244.0 / 255.0, 246.0 / 255.0, 248.0 / 255.0],
        header: [232.0 / 255.0, 236.0 / 255.0, 241.0 / 255.0],
        ui_edge: [197.0 / 255.0, 204.0 / 255.0, 213.0 / 255.0],
        ink: [37.0 / 255.0, 43.0 / 255.0, 50.0 / 255.0],
        mute: [105.0 / 255.0, 115.0 / 255.0, 127.0 / 255.0],
        accent: [102.0 / 255.0, 84.0 / 255.0, 199.0 / 255.0],
        grid_fine: [197.0 / 255.0, 206.0 / 255.0, 215.0 / 255.0],
        grid_major: [170.0 / 255.0, 182.0 / 255.0, 194.0 / 255.0],
        body: [159.0 / 255.0, 179.0 / 255.0, 197.0 / 255.0],
        body_selected: [195.0 / 255.0, 93.0 / 255.0, 42.0 / 255.0],
        body_tool: [210.0 / 255.0, 160.0 / 255.0, 75.0 / 255.0],
        body_selected_edge: [184.0 / 255.0, 50.0 / 255.0, 0.0],
        face_hover: [46.0 / 255.0, 114.0 / 255.0, 197.0 / 255.0],
        face_selected: [195.0 / 255.0, 79.0 / 255.0, 24.0 / 255.0],
        edge: [67.0 / 255.0, 81.0 / 255.0, 94.0 / 255.0],
        edge_hover: [0.0, 79.0 / 255.0, 216.0 / 255.0],
        edge_selected: [184.0 / 255.0, 50.0 / 255.0, 0.0],
        pick_halo: [23.0 / 255.0, 33.0 / 255.0, 43.0 / 255.0],
        origin_plane_xy: [11.0 / 255.0, 99.0 / 255.0, 182.0 / 255.0],
        origin_plane_xz: [37.0 / 255.0, 121.0 / 255.0, 66.0 / 255.0],
        origin_plane_yz: [181.0 / 255.0, 50.0 / 255.0, 58.0 / 255.0],
        active_sketch: [56.0 / 255.0, 86.0 / 255.0, 106.0 / 255.0],
        defined_sketch: [37.0 / 255.0, 43.0 / 255.0, 50.0 / 255.0],
        hover: [0.0, 79.0 / 255.0, 216.0 / 255.0],
        selection: [184.0 / 255.0, 50.0 / 255.0, 0.0],
        constraint_related: [15.0 / 255.0, 143.0 / 255.0, 107.0 / 255.0],
        finished_sketch: [56.0 / 255.0, 86.0 / 255.0, 106.0 / 255.0],
        finished_sketch_point: [56.0 / 255.0, 86.0 / 255.0, 106.0 / 255.0],
        finished_sketch_point_outline: [1.0, 1.0, 1.0],
        preview: [20.0 / 255.0, 127.0 / 255.0, 190.0 / 255.0],
        projected: [123.0 / 255.0, 63.0 / 255.0, 196.0 / 255.0],
        dimension: [52.0 / 255.0, 70.0 / 255.0, 0.0],
    }
}
