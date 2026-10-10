//! The retained native profile-feature panel. Every field comes from the typed form;
//! its actual widget is the control inspected and driven by MCP.

use super::{FeatureCommand, FeatureControl};
use crate::native_viewport::{
    interface_shell::ribbon::{self, Icon},
    interface_shell::{
        self, fields, InterfaceCamera, InterfaceControl, InterfaceOccluder, NativeInterfaceHandle,
    },
    ui::{ViewportUiAssets, ViewportUiTheme},
};
use crate::session_bridge::native_interface::{bind_command, NativeCommand};
use bevy::{
    ecs::system::SystemState,
    prelude::*,
    text::{FontWeight, LetterSpacing},
    ui::{BackgroundGradient, ColorStop, LinearGradient},
};
use limo_cad_interface::{DocumentContext, Field, KeyChord, Rect as Area};
use std::collections::{HashMap, HashSet};

mod overlays;
mod selection;
mod translated;

#[derive(Resource, Default)]
struct PanelWidgets {
    owner: Option<DocumentContext>,
    form_id: u64,
    root: Option<Entity>,
    header: Option<Entity>,
    header_icon: Option<Entity>,
    body: Option<Entity>,
    footer: Option<Entity>,
    controls: HashMap<String, (Entity, FeatureCommand)>,
    labels: HashMap<String, Entity>,
    area: Area,
    scroll: f32,
    max_scroll: f32,
    focus_signature: Option<String>,
    decorations: HashMap<String, Entity>,
}

fn node(x: f32, y: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(width),
        height: px(height),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

pub(crate) fn scroll_by(world: &mut World, delta: f32) -> Result<(), String> {
    let mut state = world
        .get_resource_mut::<PanelWidgets>()
        .ok_or("The feature panel is not rendered")?;
    state.scroll = (state.scroll + delta).clamp(0., state.max_scroll);
    Ok(())
}

pub(super) fn reveal_error(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<PanelWidgets>() {
        state.scroll = 0.;
    }
}

/// Choices expand inline inside the clipped body. Reveal their retained
/// selector and rows before the next layout, without moving the footer.
pub(super) fn reveal_choices(
    world: &mut World,
    owner: &DocumentContext,
    form_id: u64,
    field: super::SolidField,
    count: usize,
) {
    let Some(state) = world.get_resource::<PanelWidgets>() else {
        return;
    };
    if state.owner.as_ref() != Some(owner) || state.form_id != form_id {
        return;
    }
    let Some((entity, _)) = state.controls.get(&format!("{field:?}")) else {
        return;
    };
    let Some(Node {
        top: Val::Px(top), ..
    }) = world.get::<Node>(*entity)
    else {
        return;
    };
    let Some(Node {
        height: Val::Px(height),
        ..
    }) = state.body.and_then(|body| world.get::<Node>(body))
    else {
        return;
    };
    let selector = *top + state.scroll;
    let options_height = count as f32 * 30.;
    let bottom = selector + 40. + options_height;
    // Long lists stay scrollable; reveal the selector and the first rows.
    let next = if options_height + 60. > *height {
        (selector - 20.).max(0.)
    } else if bottom - state.scroll > *height {
        (bottom - *height).max(0.)
    } else {
        state.scroll
    };
    let max_scroll = state.max_scroll + options_height;
    world.resource_mut::<PanelWidgets>().scroll = next.clamp(0., max_scroll);
}

/// Wheel coordinates are logical window pixels, just like the published
/// panel. The controller must call this before orbit/zoom or canvas gestures.
pub(crate) fn scroll_panel(world: &mut World, point: [f32; 2], delta: f32) -> bool {
    let Some(mut state) = world.get_resource_mut::<PanelWidgets>() else {
        return false;
    };
    if state.root.is_none() || !delta.is_finite() || !point.iter().all(|v| v.is_finite()) {
        return false;
    }
    let a = state.area;
    if f64::from(point[0]) < a.x
        || f64::from(point[0]) > a.x + a.width
        || f64::from(point[1]) < a.y
        || f64::from(point[1]) > a.y + a.height
    {
        return false;
    }
    state.scroll = (state.scroll - delta).clamp(0., state.max_scroll);
    true
}

pub(crate) fn synchronize_panel(
    world: &mut World,
    _handle: &NativeInterfaceHandle,
    owner: &DocumentContext,
    area: Area,
) -> Result<(), String> {
    let panel = super::panel(world);
    synchronize_snapshot(world, owner, area, panel)
}

/// The visual lab renders the same typed panel snapshot as the desktop.
pub(crate) fn synchronize_snapshot(
    world: &mut World,
    owner: &DocumentContext,
    area: Area,
    panel: Option<super::FeaturePanel>,
) -> Result<(), String> {
    let mut state = world.remove_resource::<PanelWidgets>().unwrap_or_default();
    let result = synchronize_owned(world, owner, area, &mut state, panel);
    world.insert_resource(state);
    result
}

fn synchronize_owned(
    world: &mut World,
    owner: &DocumentContext,
    area: Area,
    state: &mut PanelWidgets,
    panel: Option<super::FeaturePanel>,
) -> Result<(), String> {
    if state.owner.as_ref() != Some(owner)
        || panel.as_ref().map(|p| p.form_id) != Some(state.form_id)
    {
        if let Some(root) = state.root {
            world.despawn(root);
        }
        *state = PanelWidgets::default();
    }
    let Some(mut panel) = panel else {
        return Ok(());
    };
    translated::apply(world, &mut panel);
    if [area.x, area.y, area.width, area.height]
        .iter()
        .any(|v| !v.is_finite())
        || area.width < 120.
        || area.height < 120.
    {
        return Err("The native feature panel needs usable window bounds".into());
    }
    let mut cameras = world.query_filtered::<Entity, With<InterfaceCamera>>();
    let camera = cameras
        .single(world)
        .map_err(|_| "Native interface camera is unavailable")?;
    let assets = world
        .get_resource::<ViewportUiAssets>()
        .cloned()
        .unwrap_or_default();
    let theme = crate::native_viewport::ui::theme(world);
    let width = area.width as f32;
    let scrolling = content_height(&panel, width - 24.) > area.height as f32 - 84.;
    let inner = width - 24. - if scrolling { 24. } else { 0. };
    let height = (area.height as f32).min(content_height(&panel, inner) + 84.);
    let body_height = height - 84.;
    state.max_scroll = (content_height(&panel, inner) - body_height).max(0.);
    state.scroll = state.scroll.min(state.max_scroll);
    let root = *state.root.get_or_insert_with(|| {
        world
            .spawn((
                Name::new("Solid feature panel"),
                Node::default(),
                BackgroundColor(theme.panel.with_alpha(1.)),
                BorderColor::all(theme.accent.with_alpha(0.78)),
                bevy::ui::BoxShadow::new(theme.dialog_shadow, px(0), px(18), px(0), px(48)),
                UiTargetCamera(camera),
                InterfaceOccluder,
                ZIndex(40),
            ))
            .id()
    });
    let mut root_node = node(area.x as f32, area.y as f32, width, height);
    root_node.border = UiRect::all(px(2.));
    root_node.border_radius = BorderRadius::all(px(14.));
    if world.get::<Node>(root) != Some(&root_node) {
        world.entity_mut(root).insert(root_node);
    }
    let fill = BackgroundColor(theme.panel.with_alpha(1.));
    let edge = BorderColor::all(theme.accent.with_alpha(0.78));
    let shadow = bevy::ui::BoxShadow::new(theme.dialog_shadow, px(0), px(18), px(0), px(48));
    if world.get::<BackgroundColor>(root) != Some(&fill) {
        world.entity_mut(root).insert(fill);
    }
    if world.get::<BorderColor>(root) != Some(&edge) {
        world.entity_mut(root).insert(edge);
    }
    if world.get::<bevy::ui::BoxShadow>(root) != Some(&shadow) {
        world.entity_mut(root).insert(shadow);
    }
    let header = *state.header.get_or_insert_with(|| {
        let header = world
            .spawn((
                Name::new("Solid feature panel header"),
                Node::default(),
                UiTargetCamera(camera),
                ZIndex(41),
            ))
            .id();
        world.entity_mut(root).add_child(header);
        header
    });
    let header_node = Node {
        position_type: PositionType::Absolute,
        left: px(0.),
        right: px(0.),
        top: px(0.),
        height: px(40.),
        border: UiRect::bottom(px(1.)),
        border_radius: BorderRadius::px(12., 12., 0., 0.),
        ..default()
    };
    if world.get::<Node>(header) != Some(&header_node) {
        world.entity_mut(header).insert(header_node);
    }
    let header_fill = BackgroundColor(theme.header);
    if world.get::<BackgroundColor>(header) != Some(&header_fill) {
        world.entity_mut(header).insert(header_fill);
    }
    let wash = BackgroundGradient::from(LinearGradient::to_right(vec![
        ColorStop::percent(ribbon::css_mix(theme.accent, theme.header, 0.12), 0.),
        ColorStop::percent(theme.header, 58.),
    ]));
    let header_edge = BorderColor::all(theme.edge);
    if world.get::<BackgroundGradient>(header) != Some(&wash) {
        world.entity_mut(header).insert(wash);
    }
    if world.get::<BorderColor>(header) != Some(&header_edge) {
        world.entity_mut(header).insert(header_edge);
    }
    let icon = header_icon(panel.kind);
    let header_icon = *state.header_icon.get_or_insert_with(|| {
        let entity = ribbon::decoration(world, camera, icon, theme.accent);
        world.entity_mut(header).add_child(entity);
        entity
    });
    ribbon::refresh_decoration(world, header_icon, icon, theme.accent);
    world
        .entity_mut(header_icon)
        .insert(node(12., 12., 15., 15.));
    let body = *state.body.get_or_insert_with(|| {
        let body = world
            .spawn((
                Name::new("feature fields"),
                Node::default(),
                UiTargetCamera(camera),
                ZIndex(41),
            ))
            .id();
        world.entity_mut(root).add_child(body);
        body
    });
    let mut body_node = node(12., 40., width - 24., body_height);
    body_node.overflow = Overflow::clip();
    if world.get::<Node>(body) != Some(&body_node) {
        world.entity_mut(body).insert(body_node);
    }
    state.owner = Some(owner.clone());
    state.form_id = panel.form_id;
    state.area = Area {
        height: f64::from(height),
        ..area
    };
    let mut live_controls = HashSet::new();
    let mut live_labels = HashSet::new();
    label(
        world,
        state,
        &mut live_labels,
        "title",
        root,
        camera,
        &panel.title,
        node(35., 8., width - 77., 24.),
        theme,
        &assets,
        true,
    );
    let mut y = 12.;
    if let Some(message) = panel.error.as_deref() {
        label(
            world,
            state,
            &mut live_labels,
            "engine-error",
            body,
            camera,
            message,
            node(0., y - state.scroll, inner, 52.),
            theme,
            &assets,
            false,
        );
        y += 56.;
    }
    let mut close =
        InterfaceControl::button(panel.kind.group(), format!("Close {}", panel.kind.label()));
    if crate::native_viewport::localization::locale(world) != crate::app_preferences::Locale::En {
        close.label = format!(
            "{}: {}",
            crate::native_viewport::localization::translate(world, "file.cancel"),
            panel.title
        );
    }
    close.disabled = panel.busy;
    widget(
        world,
        state,
        &mut live_controls,
        "close",
        root,
        camera,
        close,
        node(width - 34., 7., 26., 26.),
        FeatureCommand::Control {
            form_id: panel.form_id,
            action: FeatureControl::Cancel,
        },
        theme,
        &assets,
    )?;
    for row in panel.fields.iter().filter(|row| row.visible) {
        let key = format!("{:?}", row.field);
        if let Some((title, index, columns)) = compact_row(row.field, &panel) {
            if index != 0 {
                continue;
            }
            if !title.is_empty() {
                label(
                    world,
                    state,
                    &mut live_labels,
                    &format!("{key}-group-label"),
                    body,
                    camera,
                    translated::group(world, title),
                    node(0., y - state.scroll, inner, 18.),
                    theme,
                    &assets,
                    false,
                );
                y += 20.;
            }
            let column_width = (inner - 8. * (columns.len() - 1) as f32) / columns.len() as f32;
            let mut error = None;
            for (i, field) in columns.iter().enumerate() {
                let column = panel
                    .fields
                    .iter()
                    .find(|r| r.field == *field && r.visible)
                    .ok_or("Incomplete compact feature row")?;
                let key = format!("{field:?}");
                let x = i as f32 * (column_width + 8.);
                label(
                    world,
                    state,
                    &mut live_labels,
                    &format!("{key}-label"),
                    body,
                    camera,
                    if title.is_empty() {
                        &column.label
                    } else {
                        ["X", "Y", "Z"][i]
                    },
                    node(x, y - state.scroll, column_width, 18.),
                    theme,
                    &assets,
                    false,
                );
                let mut c = InterfaceControl::button(panel.kind.group(), &column.label);
                c.disabled = !column.enabled;
                c.field = column.value.clone();
                describe_choice(&mut c, column, panel.choice_field);
                widget(
                    world,
                    state,
                    &mut live_controls,
                    &key,
                    body,
                    camera,
                    c,
                    node(x, y + 20. - state.scroll, column_width, 30.),
                    FeatureCommand::Control {
                        form_id: panel.form_id,
                        action: FeatureControl::Field(*field),
                    },
                    theme,
                    &assets,
                )?;
                numeric_steps(
                    world,
                    state,
                    &mut live_controls,
                    body,
                    camera,
                    &panel,
                    column,
                    &key,
                    x,
                    y + 20. - state.scroll,
                    column_width,
                    theme,
                    &assets,
                )?;
                error = error.or(column.error.as_deref());
            }
            y += 56.;
            for column in panel
                .fields
                .iter()
                .filter(|r| columns.contains(&r.field) && r.visible)
            {
                choice_options(
                    world,
                    state,
                    &mut live_controls,
                    body,
                    camera,
                    &panel,
                    column,
                    inner,
                    &mut y,
                    theme,
                    &assets,
                )?;
            }
            if let Some(error) = error {
                label(
                    world,
                    state,
                    &mut live_labels,
                    &format!("{key}-error"),
                    body,
                    camera,
                    error,
                    node(0., y - state.scroll, inner, 38.),
                    theme,
                    &assets,
                    false,
                );
                y += 42.;
            }
            continue;
        }
        if matches!(
            row.field,
            super::SolidField::Axis | super::SolidField::MoveObjectType
        ) || (row.field == super::SolidField::Operation
            && panel.kind == super::SolidFormKind::Extrude)
        {
            if let Field::Choice { value, options } = &row.value {
                label(
                    world,
                    state,
                    &mut live_labels,
                    &format!("{key}-radio-label"),
                    body,
                    camera,
                    &row.label.to_uppercase(),
                    node(0., y - state.scroll, inner, 18.),
                    theme,
                    &assets,
                    false,
                );
                y += 20.;
                for (index, option) in options.iter().enumerate() {
                    let mut c = InterfaceControl::button(panel.kind.group(), &option.label);
                    c.role = "radio".into();
                    c.selected = Some(option.value == *value);
                    c.disabled = !row.enabled || option.disabled;
                    widget(
                        world,
                        state,
                        &mut live_controls,
                        &format!("{key}-{}", option.value),
                        body,
                        camera,
                        c,
                        node(
                            (index % 2) as f32 * (inner + 6.) * 0.5,
                            y + (index / 2) as f32 * 36. - state.scroll,
                            (inner - 6.) * 0.5,
                            32.,
                        ),
                        FeatureCommand::Control {
                            form_id: panel.form_id,
                            action: FeatureControl::Choose {
                                field: row.field,
                                option: index,
                            },
                        },
                        theme,
                        &assets,
                    )?;
                }
                y += options.len().div_ceil(2) as f32 * 36. + 4.;
                if row.field == super::SolidField::Operation {
                    for (key, hint) in [
                        ("operation-hint", panel.presentation.operation_hint),
                        ("automatic-hint", panel.presentation.automatic_hint),
                    ] {
                        if let Some(hint) = hint {
                            let text = crate::native_viewport::localization::translate(world, hint);
                            let h = selection::text_height(text, inner);
                            label(
                                world,
                                state,
                                &mut live_labels,
                                key,
                                body,
                                camera,
                                text,
                                node(0., y - state.scroll, inner, h),
                                theme,
                                &assets,
                                false,
                            );
                            world.entity_mut(state.labels[key]).insert((
                                theme.text(&assets, 10., FontWeight::NORMAL),
                                bevy::text::LineHeight::Px(16.),
                            ));
                            if key == "automatic-hint" {
                                world
                                    .entity_mut(state.labels[key])
                                    .insert(TextColor(theme.accent));
                            }
                            y += h + 4.;
                        }
                    }
                }

                continue;
            }
        }
        if matches!(row.value, Field::None) {
            if row.field.is_hole_position_action() {
                let mut control = InterfaceControl::button(panel.kind.group(), &row.label);
                control.disabled = !row.enabled;
                widget(
                    world,
                    state,
                    &mut live_controls,
                    &key,
                    body,
                    camera,
                    control,
                    node(0., y - state.scroll, inner, 32.),
                    FeatureCommand::Control {
                        form_id: panel.form_id,
                        action: FeatureControl::Field(row.field),
                    },
                    theme,
                    &assets,
                )?;
                y += 40.;
                continue;
            }
            selection::render(
                world,
                state,
                &mut live_controls,
                &mut live_labels,
                body,
                camera,
                &panel,
                row,
                inner,
                &mut y,
                theme,
                &assets,
            )?;
            continue;
        }
        if row.field == super::SolidField::Copy && panel.kind == super::SolidFormKind::MoveCopy {
            overlays::copy_card(
                world,
                state,
                &mut live_controls,
                &mut live_labels,
                body,
                camera,
                &panel,
                row,
                inner,
                &mut y,
                theme,
                &assets,
            )?;
            continue;
        }
        let mut control = InterfaceControl::button(panel.kind.group(), &row.label);
        control.disabled = !row.enabled;
        control.field = row.value.clone();
        let action;
        match &row.value {
            Field::Text { .. } | Field::Choice { .. } | Field::Range { .. } => {
                label(
                    world,
                    state,
                    &mut live_labels,
                    &format!("{key}-label"),
                    body,
                    camera,
                    &row.label,
                    node(0., y - state.scroll, inner, 18.),
                    theme,
                    &assets,
                    false,
                );
                y += 20.;
                action = FeatureControl::Field(row.field);
                describe_choice(&mut control, row, panel.choice_field);
            }
            Field::Toggle(value) => {
                action = FeatureControl::Field(row.field);
                control.role = "checkbox".into();
                control.selected = Some(*value);
            }
            Field::None => unreachable!(),
        }
        widget(
            world,
            state,
            &mut live_controls,
            &key,
            body,
            camera,
            control,
            node(0., y - state.scroll, inner, 28.),
            FeatureCommand::Control {
                form_id: panel.form_id,
                action,
            },
            theme,
            &assets,
        )?;
        numeric_steps(
            world,
            state,
            &mut live_controls,
            body,
            camera,
            &panel,
            row,
            &key,
            0.,
            y - state.scroll,
            inner,
            theme,
            &assets,
        )?;
        y += 40.;
        choice_options(
            world,
            state,
            &mut live_controls,
            body,
            camera,
            &panel,
            row,
            inner,
            &mut y,
            theme,
            &assets,
        )?;
        if let Some(error) = &row.error {
            label(
                world,
                state,
                &mut live_labels,
                &format!("{key}-error"),
                body,
                camera,
                error,
                node(0., y - state.scroll, inner, 38.),
                theme,
                &assets,
                false,
            );
            y += 42.;
        }
    }
    for (i, message) in panel.notes.iter().enumerate() {
        label(
            world,
            state,
            &mut live_labels,
            &format!("feature-note-{i}"),
            body,
            camera,
            message,
            node(0., y - state.scroll, inner, note_height(message) - 4.),
            theme,
            &assets,
            false,
        );
        y += note_height(message);
    }
    if let Some(message) = panel.preview_notice.as_deref() {
        label(
            world,
            state,
            &mut live_labels,
            "preview-notice",
            body,
            camera,
            message,
            node(0., y - state.scroll, inner, 52.),
            theme,
            &assets,
            false,
        );
        y += 56.;
    }
    state.max_scroll = (y - body_height).max(0.);
    state.scroll = state.scroll.min(state.max_scroll);
    if state.max_scroll > 0. {
        for (key, caption, delta, y, disabled) in [
            (
                "scroll-up",
                "Scroll feature up",
                -180,
                42.,
                state.scroll <= 0.,
            ),
            (
                "scroll-down",
                "Scroll feature down",
                180,
                height - 76.,
                state.scroll >= state.max_scroll,
            ),
        ] {
            let mut control = InterfaceControl::button(panel.kind.group(), caption);
            control.disabled = panel.busy || disabled;
            widget(
                world,
                state,
                &mut live_controls,
                key,
                root,
                camera,
                control,
                node(width - 30., y, 20., 22.),
                FeatureCommand::Control {
                    form_id: panel.form_id,
                    action: FeatureControl::Scroll(delta),
                },
                theme,
                &assets,
            )?;
        }
    }
    let footer = *state.footer.get_or_insert_with(|| {
        let entity = world
            .spawn((
                Name::new("Solid feature panel footer"),
                UiTargetCamera(camera),
                ZIndex(41),
            ))
            .id();
        world.entity_mut(root).add_child(entity);
        entity
    });
    let mut footer_node = node(0., height - 44., width - 4., 44.);
    footer_node.border = UiRect::top(px(1.));
    footer_node.border_radius = BorderRadius::px(0., 0., 12., 12.);
    world.entity_mut(footer).insert((
        footer_node,
        BackgroundColor(theme.header),
        BorderColor::all(theme.edge),
    ));
    let apply_width =
        footer_button_width(translated::caption(world, "apply").unwrap_or("OK"), width);
    let cancel_width = footer_button_width(
        translated::caption(world, "cancel").unwrap_or("Cancel"),
        width,
    );
    let apply_x = width - 16. - apply_width;
    let mut cancel =
        InterfaceControl::button(panel.kind.group(), format!("Cancel {}", panel.kind.label()));
    if let Some(caption) = translated::caption(world, "cancel") {
        cancel.label = format!("{caption}: {}", panel.title);
    }
    cancel.disabled = panel.busy;
    widget(
        world,
        state,
        &mut live_controls,
        "cancel",
        root,
        camera,
        cancel,
        node(apply_x - 8. - cancel_width, height - 36., cancel_width, 28.),
        FeatureCommand::Control {
            form_id: panel.form_id,
            action: FeatureControl::Cancel,
        },
        theme,
        &assets,
    )?;
    let mut apply = InterfaceControl::button(
        panel.kind.group(),
        if panel.busy {
            "Applying…".to_owned()
        } else {
            format!("Apply {}", panel.kind.label())
        },
    );
    if let Some(caption) = translated::caption(world, "apply") {
        apply.label = format!("{caption}: {}", panel.title);
    }
    apply.disabled = !panel.can_apply;
    apply.selected = Some(true);
    widget(
        world,
        state,
        &mut live_controls,
        "apply",
        root,
        camera,
        apply,
        node(apply_x, height - 36., apply_width, 28.),
        FeatureCommand::Control {
            form_id: panel.form_id,
            action: FeatureControl::Apply,
        },
        theme,
        &assets,
    )?;
    floating_distance(
        world,
        state,
        &mut live_controls,
        &mut live_labels,
        root,
        camera,
        &panel,
        area,
        theme,
        &assets,
    )?;
    overlays::selection_prompt(
        world,
        state,
        &mut live_labels,
        root,
        camera,
        &panel,
        area,
        theme,
        &assets,
    );
    if let Some((field, signature)) = &panel.presentation.auto_focus {
        if state.focus_signature.as_ref() != Some(signature) {
            if let Some((entity, _)) = state.controls.get(&format!("{field:?}")) {
                fields::request_focus(world, *entity, owner);
                state.focus_signature = Some(signature.clone());
            }
        }
    } else {
        state.focus_signature = None;
    }
    state.decorations.retain(|key, entity| {
        if live_labels.contains(key) {
            true
        } else {
            if world.get_entity(*entity).is_ok() {
                world.despawn(*entity);
            }
            false
        }
    });
    state.controls.retain(|key, (entity, _)| {
        if live_controls.contains(key) {
            true
        } else {
            world.despawn(*entity);
            false
        }
    });
    if panel.kind == super::SolidFormKind::MoveCopy {
        use super::SolidField as F;
        for field in [
            F::TranslationX,
            F::TranslationY,
            F::TranslationZ,
            F::RotationX,
            F::RotationY,
            F::RotationZ,
            F::PivotX,
            F::PivotY,
            F::PivotZ,
            F::FromX,
            F::FromY,
            F::FromZ,
            F::ToX,
            F::ToY,
            F::ToZ,
        ] {
            if let Some((entity, _)) = state.controls.get(&format!("{field:?}")) {
                fields::compact_number_caption(world, *entity, theme, &assets);
            }
        }
    }
    state.labels.retain(|key, entity| {
        if live_labels.contains(key) {
            true
        } else {
            world.despawn(*entity);
            false
        }
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn widget(
    world: &mut World,
    state: &mut PanelWidgets,
    live: &mut HashSet<String>,
    key: &str,
    parent: Entity,
    camera: Entity,
    mut control: InterfaceControl,
    mut node: Node,
    command: FeatureCommand,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) -> Result<(), String> {
    live.insert(key.into());
    node.border = UiRect::all(px(1.));
    node.border_radius = BorderRadius::all(px(4.));
    node.padding = UiRect::axes(px(7.), px(3.));
    if matches!(control.field, Field::Text { .. }) {
        if let FeatureCommand::Control {
            action: FeatureControl::Field(field),
            ..
        } = command
        {
            if field.dimension_kind().is_some() {
                node.padding.right = px(22.);
            }
        }
        control.owned_keys = vec![KeyChord::plain("Enter")];
    }
    let entity = if let Some((entity, _)) = state.controls.get(key) {
        *entity
    } else {
        let mut system = SystemState::<Commands>::new(world);
        let entity = {
            let mut commands = system.get_mut(world).map_err(|e| e.to_string())?;
            if matches!(control.field, Field::Range { .. }) {
                interface_shell::ranges::spawn(
                    &mut commands,
                    camera,
                    node.clone(),
                    control.clone(),
                    theme,
                )
            } else if matches!(control.field, Field::Text { .. }) {
                fields::spawn_text_field(
                    &mut commands,
                    camera,
                    node.clone(),
                    control.clone(),
                    theme,
                    assets,
                )?
            } else {
                interface_shell::spawn_button(
                    &mut commands,
                    camera,
                    node.clone(),
                    control.clone(),
                    theme,
                    assets,
                )
            }
        };
        system.apply(world);
        world.entity_mut(parent).add_child(entity);
        if matches!(control.field, Field::Choice { .. }) {
            let glyph = interface_shell::ribbon::compact_glyph(
                world,
                entity,
                interface_shell::ribbon::Icon::Chevron,
                0.,
                10.,
            );
            let mut bounds = world.get::<Node>(glyph).unwrap().clone();
            bounds.left = Val::Auto;
            bounds.right = px(8.);
            bounds.top = px(10.);
            world.entity_mut(glyph).insert(bounds);
        }
        if key == "close" {
            ribbon::compact_glyph(world, entity, Icon::Cancel, 0., 14.);
        }
        if key.ends_with("-step-up") || key.ends_with("-step-down") {
            let glyph = ribbon::compact_glyph(world, entity, Icon::Chevron, 0., 10.);
            if key.ends_with("-step-up") {
                world
                    .entity_mut(glyph)
                    .insert(bevy::ui::UiTransform::from_rotation(Rot2::PI));
            }
        }
        bind_command(world, entity, NativeCommand::Feature(command.clone()))?;
        state.controls.insert(key.into(), (entity, command.clone()));
        entity
    };
    if state.controls[key].1 != command {
        bind_command(world, entity, NativeCommand::Feature(command.clone()))?;
        state.controls.get_mut(key).unwrap().1 = command;
    }
    control.binding = world
        .get::<InterfaceControl>(entity)
        .ok_or("Feature widget was removed")?
        .binding;
    if matches!(control.field, Field::Text { .. }) {
        control.text_editing = true;
        control.role = "textbox".into();
        world.entity_mut(entity).insert(fields::LiveValue);
    }
    if matches!(control.field, Field::Range { .. }) {
        control.role = "slider".into();
        control.owned_keys = [
            "ArrowLeft",
            "ArrowRight",
            "ArrowUp",
            "ArrowDown",
            "Home",
            "End",
        ]
        .map(KeyChord::plain)
        .into();
    }
    if world.get::<InterfaceControl>(entity) != Some(&control) {
        world.entity_mut(entity).insert(control);
    }
    if world.get::<Node>(entity) != Some(&node) {
        world.entity_mut(entity).insert(node);
    }
    if world.get::<ZIndex>(entity) != Some(&ZIndex(42)) {
        world.entity_mut(entity).insert(ZIndex(42));
    }
    if key == "apply" {
        interface_shell::primary_button(world, entity);
    }
    if world
        .get::<InterfaceControl>(entity)
        .is_some_and(|control| control.role == "radio")
    {
        interface_shell::reference_button(world, entity);
        interface_shell::caption_weight(world, entity, FontWeight::NORMAL);
    }
    let caption = if key == "close" {
        Some("")
    } else if key == "apply" {
        Some("OK")
    } else if key == "cancel" {
        Some("Cancel")
    } else if key == "scroll-up" {
        Some("↑")
    } else if key == "scroll-down" {
        Some("↓")
    } else if key.ends_with("-clear") {
        Some("Clear")
    } else if key.ends_with("-step-up") || key.ends_with("-step-down") {
        Some("")
    } else {
        None
    };
    let caption = translated::caption(world, key).or(caption);
    if let Some(text) = caption {
        let caption = interface_shell::InterfaceCaption(text.into());
        if world.get::<interface_shell::InterfaceCaption>(entity) != Some(&caption) {
            world.entity_mut(entity).insert(caption);
        }
    }
    if key == "apply" {
        interface_shell::primary_button(world, entity);
        interface_shell::caption_weight(world, entity, FontWeight::SEMIBOLD);
    }
    if matches!(key, "apply" | "cancel") {
        interface_shell::caption_size(world, entity, 12.);
        interface_shell::center_caption(world, entity);
    }
    if key == "cancel" {
        interface_shell::caption_weight(world, entity, FontWeight::NORMAL);
    }
    if key == "close" {
        world
            .entity_mut(entity)
            .insert(interface_shell::InterfaceFlat);
        ribbon::center_glyph(world, entity);
    }
    if key.ends_with("-step-up") || key.ends_with("-step-down") {
        world
            .entity_mut(entity)
            .insert(interface_shell::InterfaceFlat);
        ribbon::center_glyph(world, entity);
        if let Some(handle) = world.get_resource::<NativeInterfaceHandle>() {
            handle.exclude_from_tab(limo_cad_interface::ControlKey(entity.to_bits()))?;
        }
    }
    if let Some(control) = world.get::<InterfaceControl>(entity) {
        if let Field::Toggle(checked) = control.field {
            interface_shell::checkbox_button(world, entity, camera, checked);
            let mut bounds = world.get::<Node>(entity).unwrap().clone();
            bounds.justify_content = JustifyContent::Start;
            bounds.border = UiRect::default();
            world.entity_mut(entity).insert(bounds);
        } else if let Field::Choice { value, options } = &control.field {
            let selected = options
                .iter()
                .find(|option| option.value == *value)
                .map(|option| option.label.as_str())
                .unwrap_or(value);
            let caption = interface_shell::InterfaceCaption(selected.into());
            if world.get::<interface_shell::InterfaceCaption>(entity) != Some(&caption) {
                world.entity_mut(entity).insert(caption);
            }
            if key == "HolePositionSelection" {
                interface_shell::clip_caption(world, entity, 8., 24.);
            } else {
                interface_shell::caption_node(
                    world,
                    entity,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(8.),
                        right: px(24.),
                        top: px(5.),
                        height: px(20.),
                        ..default()
                    },
                );
            }
            interface_shell::caption_weight(world, entity, FontWeight::NORMAL);
        }
    }
    if key.starts_with("HolePositionSelection-option-") {
        interface_shell::clip_caption(world, entity, 8., 8.);
    }
    Ok(())
}

pub(super) fn focus_measurement(
    world: &mut World,
    owner: &DocumentContext,
    field: super::SolidField,
) {
    let focused = world
        .get_resource::<NativeInterfaceHandle>()
        .and_then(|h| h.focused_key());
    let entity = world.get_resource::<PanelWidgets>().and_then(|state| {
        let canvas = state.controls.iter().any(|(key, (e, _))| {
            (key.starts_with("extrude-distance-step") || key.starts_with("offset-distance-step"))
                && focused == Some(limo_cad_interface::ControlKey(e.to_bits()))
        });
        let key = if canvas {
            if state.controls.contains_key("extrude-distance") {
                "extrude-distance".into()
            } else {
                "offset-distance".into()
            }
        } else {
            format!("{field:?}")
        };
        state.controls.get(&key).map(|(entity, _)| *entity)
    });
    if let Some(entity) = entity {
        fields::request_focus(world, entity, owner);
    }
}

fn compact_row(
    field: super::SolidField,
    panel: &super::FeaturePanel,
) -> Option<(&'static str, usize, &'static [super::SolidField])> {
    let row = field.compact_row(panel.kind)?;
    row.2
        .iter()
        .all(|f| panel.fields.iter().any(|r| r.field == *f && r.visible))
        .then_some(row)
}

fn visible_error<'a>(
    panel: &super::FeaturePanel,
    row: &'a super::SolidFieldView,
) -> Option<&'a str> {
    if matches!(row.value, Field::None)
        && !panel
            .presentation
            .references
            .get(&row.field)
            .is_some_and(|r| r.has_selection)
    {
        None
    } else {
        row.error.as_deref()
    }
}

fn operation_hints_height(panel: &super::FeaturePanel, inner: f32) -> f32 {
    use crate::app_preferences::{locale::translate, Locale};
    [
        panel.presentation.operation_hint,
        panel.presentation.automatic_hint,
    ]
    .into_iter()
    .flatten()
    .map(|key| selection::text_height(translate(Locale::En, key), inner) + 4.)
    .sum()
}

#[allow(clippy::too_many_arguments)]
fn numeric_steps(
    world: &mut World,
    state: &mut PanelWidgets,
    controls: &mut HashSet<String>,
    parent: Entity,
    camera: Entity,
    panel: &super::FeaturePanel,
    row: &super::SolidFieldView,
    key: &str,
    x: f32,
    y: f32,
    width: f32,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) -> Result<(), String> {
    if !matches!(
        row.value,
        Field::Text {
            read_only: false,
            ..
        }
    ) || row.field.dimension_kind().is_none()
    {
        return Ok(());
    }
    for (suffix, label, delta, dy) in [("up", "Increase", 1, 1.), ("down", "Decrease", -1, 14.)] {
        let name = match key {
            "extrude-distance" => "Extrude canvas distance",
            "offset-distance" => "Offset plane distance",
            _ => &row.label,
        };
        let mut control = InterfaceControl::button(panel.kind.group(), format!("{label} {name}"));
        control.disabled = !row.enabled || panel.busy;
        widget(
            world,
            state,
            controls,
            &format!("{key}-step-{suffix}"),
            parent,
            camera,
            control,
            node(x + width - 19., y + dy, 14., 12.),
            FeatureCommand::Control {
                form_id: panel.form_id,
                action: FeatureControl::Step {
                    field: row.field,
                    delta,
                },
            },
            theme,
            assets,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn floating_distance(
    world: &mut World,
    state: &mut PanelWidgets,
    controls: &mut HashSet<String>,
    labels: &mut HashSet<String>,
    root: Entity,
    camera: Entity,
    panel: &super::FeaturePanel,
    area: Area,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) -> Result<(), String> {
    let extrude = panel.kind == super::SolidFormKind::Extrude;
    let anchor = if extrude {
        super::manipulator::distance_anchor(world)
    } else {
        super::manipulator::anchor(world)
    };
    let viewport = world
        .get_resource::<NativeInterfaceHandle>()
        .and_then(|h| h.frame())
        .and_then(|frame| {
            frame
                .canvases
                .iter()
                .find(|c| c.name == "viewport")
                .map(|c| c.bounds)
        });
    let row = panel
        .fields
        .iter()
        .find(|r| r.field == super::SolidField::Distance && r.visible);
    let (Some(anchor), Some(canvas), Some(row)) = (anchor, viewport, row) else {
        return Ok(());
    };
    let frame_width = if extrude { 150. } else { 132. };
    let min_x = canvas.x as f32 + 8.;
    let x = (canvas.x as f32 + anchor[0] - if extrude { frame_width * 0.5 } else { -24. })
        .clamp(min_x, (area.x as f32 - frame_width - 12.).max(min_x));
    let min_y = canvas.y as f32 + 8.;
    let y = (canvas.y as f32 + anchor[1] - if extrude { 16. } else { -8. }).clamp(
        min_y,
        (canvas.y as f32 + canvas.height as f32 - 42.).max(min_y),
    );
    let frame_key = "distance-overlay-frame";
    labels.insert(frame_key.into());
    let frame = *state
        .decorations
        .entry(frame_key.into())
        .or_insert_with(|| {
            let e = world
                .spawn((UiTargetCamera(camera), InterfaceOccluder, ZIndex(43)))
                .id();
            world.entity_mut(root).add_child(e);
            e
        });
    let mut bounds = node(x - area.x as f32, y - area.y as f32, frame_width, 32.);
    bounds.border = UiRect::all(px(1.));
    bounds.border_radius = BorderRadius::all(px(6.));
    world.entity_mut(frame).insert((
        bounds,
        BackgroundColor(theme.header.with_alpha(0.95)),
        BorderColor::all(theme.accent),
        bevy::ui::BoxShadow::new(theme.dialog_shadow, px(0), px(5), px(0), px(12)),
    ));
    label(
        world,
        state,
        labels,
        "distance-overlay-caption",
        frame,
        camera,
        if extrude { "DISTANCE" } else { "OFFSET" },
        node(8., 7., 55., 18.),
        theme,
        assets,
        false,
    );
    label(
        world,
        state,
        labels,
        "distance-overlay-units",
        frame,
        camera,
        &panel.presentation.units,
        node(frame_width - 24., 7., 19., 18.),
        theme,
        assets,
        false,
    );
    let mut control = InterfaceControl::button(
        panel.kind.group(),
        if extrude {
            "Extrude canvas distance"
        } else {
            "Offset plane distance"
        },
    );
    control.field = row.value.clone();
    control.disabled = panel.busy;
    let key = if extrude {
        "extrude-distance"
    } else {
        "offset-distance"
    };
    let mut field_theme = theme;
    field_theme.header = Color::NONE;
    field_theme.edge = Color::NONE;
    let field_width = frame_width - 84.;
    widget(
        world,
        state,
        controls,
        key,
        frame,
        camera,
        control,
        node(61., 2., field_width, 28.),
        FeatureCommand::Control {
            form_id: panel.form_id,
            action: FeatureControl::Field(super::SolidField::Distance),
        },
        field_theme,
        assets,
    )?;
    numeric_steps(
        world,
        state,
        controls,
        frame,
        camera,
        panel,
        row,
        key,
        61.,
        2.,
        field_width,
        field_theme,
        assets,
    )?;
    Ok(())
}

fn content_height(panel: &super::FeaturePanel, inner: f32) -> f32 {
    let mut height = 24. + panel.notes.iter().map(|s| note_height(s)).sum::<f32>();
    for row in panel.fields.iter().filter(|row| row.visible) {
        if row.field.is_hole_position_action() {
            height += 40.;
            continue;
        }
        if row.field == super::SolidField::Copy && panel.kind == super::SolidFormKind::MoveCopy {
            height += overlays::copy_height(panel, inner);
            continue;
        }
        if let Some((title, index, columns)) = compact_row(row.field, panel) {
            if panel.choice_field == Some(row.field) {
                if let Field::Choice { options, .. } = &row.value {
                    height += options.len() as f32 * 30.;
                }
            }
            if index == 0 {
                height += 56. + if title.is_empty() { 0. } else { 20. };
                if panel
                    .fields
                    .iter()
                    .any(|r| columns.contains(&r.field) && r.error.is_some())
                {
                    height += 42.;
                }
            }
            continue;
        }
        if matches!(
            row.field,
            super::SolidField::Axis | super::SolidField::MoveObjectType
        ) || (row.field == super::SolidField::Operation
            && panel.kind == super::SolidFormKind::Extrude)
        {
            height += if row.field == super::SolidField::Axis {
                96.
            } else if row.field == super::SolidField::Operation {
                96. + operation_hints_height(panel, inner)
            } else {
                60.
            };
            continue;
        }
        height += match row.value {
            Field::Toggle(_) => 40.,
            Field::None => selection::height(panel, row, inner),
            _ => 60.,
        };
        if panel.choice_field == Some(row.field) {
            if let Field::Choice { options, .. } = &row.value {
                height += options.len() as f32 * 30.;
            }
        }
        if visible_error(panel, row).is_some() {
            height += 42.;
        }
    }
    if panel.error.is_some() {
        height += 56.;
    }
    if panel.preview_notice.is_some() {
        height += 56.;
    }
    height
}

#[allow(clippy::too_many_arguments)]
fn label(
    world: &mut World,
    state: &mut PanelWidgets,
    live: &mut HashSet<String>,
    key: &str,
    parent: Entity,
    camera: Entity,
    text: &str,
    node: Node,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
    strong: bool,
) {
    let field_label = key.ends_with("-label");
    let text = if field_label {
        text.to_uppercase()
    } else {
        text.to_owned()
    };
    live.insert(key.into());
    let entity = *state.labels.entry(key.into()).or_insert_with(|| {
        let entity = world
            .spawn((
                Text::new(&text),
                theme.text(
                    assets,
                    if strong {
                        12.
                    } else if field_label {
                        10.
                    } else {
                        11.
                    },
                    if strong || field_label {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    },
                ),
                TextColor(if strong { theme.ink } else { theme.mute }),
                LetterSpacing::Px(if field_label { 0.5 } else { 0. }),
                node.clone(),
                UiTargetCamera(camera),
                ZIndex(42),
            ))
            .id();
        world.entity_mut(parent).add_child(entity);
        entity
    });
    if world
        .get::<Text>(entity)
        .is_some_and(|current| current.0 != text)
    {
        world.get_mut::<Text>(entity).unwrap().0 = text;
    }
    if world.get::<Node>(entity) != Some(&node) {
        world.entity_mut(entity).insert(node);
    }
    let ink = TextColor(if strong { theme.ink } else { theme.mute });
    if world.get::<TextColor>(entity) != Some(&ink) {
        world.entity_mut(entity).insert(ink);
    }
}

fn footer_button_width(caption: &str, panel_width: f32) -> f32 {
    (caption.chars().count() as f32 * 7. + 24.)
        .max(64.)
        .min((panel_width - 40.) * 0.5)
}

fn header_icon(kind: super::SolidFormKind) -> Icon {
    use super::SolidFormKind as K;
    match kind {
        K::Extrude => Icon::Box,
        K::Revolve => Icon::RefreshCw,
        K::Sweep => Icon::MoveRight,
        K::Loft | K::OffsetPlane | K::Midplane | K::AnglePlane => Icon::Layers,
        K::Rib => Icon::PanelTop,
        K::Hole => Icon::CircleDot,
        K::Fillet => Icon::Blend,
        K::Chamfer => Icon::Triangle,
        K::ExternalThread | K::CircularPattern => Icon::RotateCw,
        K::Shell => Icon::ShellSymbol,
        K::MoveCopy => Icon::Orbit,
        K::Combine => Icon::CombineSymbol,
        K::Mirror => Icon::Copy,
        K::SplitBody => Icon::Scissors,
        K::RectangularPattern => Icon::Boxes,
    }
}

fn describe_choice(
    control: &mut InterfaceControl,
    row: &super::SolidFieldView,
    expanded: Option<super::SolidField>,
) {
    if let Field::Choice { value, options } = &row.value {
        let selected = options
            .iter()
            .find(|o| &o.value == value)
            .map(|o| o.label.as_str())
            .unwrap_or(value);
        control.label = format!("{}: {}", row.label, selected);
        control.role = "combobox".into();
        control.expanded = Some(expanded == Some(row.field));
        control.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
            .map(KeyChord::plain)
            .into();
    }
}

#[allow(clippy::too_many_arguments)]
fn choice_options(
    world: &mut World,
    state: &mut PanelWidgets,
    live: &mut HashSet<String>,
    body: Entity,
    camera: Entity,
    panel: &super::FeaturePanel,
    row: &super::SolidFieldView,
    inner: f32,
    y: &mut f32,
    theme: ViewportUiTheme,
    assets: &ViewportUiAssets,
) -> Result<(), String> {
    let key = format!("{:?}", row.field);
    if panel.choice_field == Some(row.field) {
        if let Field::Choice { value, options } = &row.value {
            for (index, option) in options.iter().enumerate() {
                let mut choice = InterfaceControl::button(panel.kind.group(), &option.label);
                choice.disabled = option.disabled || !row.enabled;
                choice.role = "option".into();
                choice.selected = Some(&option.value == value);
                widget(
                    world,
                    state,
                    live,
                    &format!("{key}-option-{}", option.value),
                    body,
                    camera,
                    choice,
                    node(8., *y - state.scroll, inner - 8., 28.),
                    FeatureCommand::Control {
                        form_id: panel.form_id,
                        action: FeatureControl::Choose {
                            field: row.field,
                            option: index,
                        },
                    },
                    theme,
                    assets,
                )?;
                *y += 30.;
            }
        }
    }

    Ok(())
}

fn note_height(message: &str) -> f32 {
    if message.chars().count() > 65 {
        46.
    } else {
        26.
    }
}
