use super::*;

pub(crate) fn synchronize(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    revision: u64,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let mut state = world.remove_resource::<History>().unwrap_or_default();
    let result = (|| {
        let receipt = DocumentReceipt {
            owner: owner.clone(),
            revision,
        };
        if state
            .snapshot
            .as_ref()
            .is_none_or(|(prior, _)| prior != &receipt)
        {
            if state
                .snapshot
                .as_ref()
                .is_none_or(|(prior, _)| prior.owner != *owner)
            {
                state.menu = None;
                state.delete = None;
                state.rename = None;
                state.selected = None;
                state.scroll = 0;
                state.drag = None;
            }
            let document = services.engine.document_snapshot();
            if state
                .selected
                .is_some_and(|id| feature(&document, id).is_err())
            {
                state.selected = None;
            }
            state.snapshot = Some((receipt, Arc::new(document)));
        }
        let document = state.snapshot.as_ref().unwrap().1.clone();
        let mut cameras = world.query_filtered::<Entity, With<InterfaceCamera>>();
        let camera = cameras
            .single(world)
            .map_err(|_| "Missing interface camera")?;
        let theme = crate::native_viewport::ui::theme(world);
        let y = (height - 48.).max(0.);
        let locked = idle(world).is_err();
        let count = document.features.len();
        let rollback = document.rollback_index;
        state.widgets.begin();
        state.widgets.panel(
            world,
            camera,
            "history-bg",
            rect(0., y, width, 48.),
            theme.panel,
            22,
        );
        state.widgets.panel(
            world,
            camera,
            "history-top-edge",
            rect(0., y, width, 1.),
            theme.edge,
            23,
        );
        super::super::workbench::card(
            (&mut state.widgets, world, camera),
            "history-title-bg",
            rect(12., y + 8., 180., 32.),
            interface_shell::ribbon::css_mix(theme.header, theme.panel, 0.55),
            8.,
            23,
        );
        state.widgets.glyph(
            (world, camera),
            "history-icon",
            rect(23., y + 17., 13., 13.),
            Icon::History,
            theme.accent,
            24,
        );
        state.widgets.text(
            world,
            camera,
            "history-title",
            rect(44., y + 17., 105., 16.),
            "DESIGN HISTORY",
            9.,
            24,
        );
        if let Some(e) = state.widgets.entity("history-title") {
            let assets = world.resource::<ViewportUiAssets>().clone();
            world.entity_mut(e).insert((
                TextColor(theme.mute),
                theme.text(&assets, 10., FontWeight::SEMIBOLD),
                bevy::text::LetterSpacing::Px(2.),
                TextLayout::no_wrap(),
            ));
        }
        state.widgets.panel(
            world,
            camera,
            "history-count-bg",
            rect(154., y + 15., 30., 18.),
            theme.panel,
            24,
        );
        state.widgets.text(
            world,
            camera,
            "history-count",
            rect(154., y + 17., 30., 14.),
            &format!("{rollback}/{count}"),
            9.,
            25,
        );
        if let Some(e) = state.widgets.entity("history-count") {
            world
                .entity_mut(e)
                .insert(TextLayout::justify(Justify::Center));
        }
        super::super::workbench::card(
            (&mut state.widgets, world, camera),
            "history-transport",
            rect(200., y + 9., 122., 30.),
            interface_shell::ribbon::css_mix(theme.header, theme.panel, 0.3),
            8.,
            23,
        );
        for (i, (label, icon, index)) in [
            ("Roll back to start", Icon::ArrowLeftToLine, 0),
            (
                "Previous feature",
                Icon::ArrowLeft,
                rollback.saturating_sub(1),
            ),
            ("Next feature", Icon::ArrowRight, (rollback + 1).min(count)),
            ("Roll forward to end", Icon::ArrowRightToLine, count),
        ]
        .into_iter()
        .enumerate()
        {
            state.widgets.button(
                world,
                camera,
                &format!("rollback-{i}"),
                control(label, None, locked || index == rollback),
                Some(""),
                NativeCommand::History(HistoryCommand::Rollback(index)),
                rect(203. + i as f32 * 29., y + 12., 28., 24.),
                Some(icon),
                24,
            )?;
        }
        let list_x = 330.;
        let available = (width - list_x - 42.).max(1.);
        state.scroll = state.scroll.min(count.saturating_sub(1));
        super::super::workbench::card(
            (&mut state.widgets, world, camera),
            "history-strip",
            rect(list_x, y + 6., width - list_x - 12., 36.),
            interface_shell::ribbon::css_mix(theme.header, theme.panel, 0.25),
            8.,
            23,
        );
        state.widgets.panel(
            world,
            camera,
            "history-track",
            rect(
                list_x + 8.,
                y + 23.,
                width - list_x - 28.,
                if count == 0 { 4. } else { 1. },
            ),
            theme.edge,
            23,
        );
        let mut x = list_x + 8.;
        let mut shown = 0;
        let mut marker_x = None;
        for (index, feature) in document.features.iter().enumerate().skip(state.scroll) {
            let w = (feature.name.chars().count() as f32 * 5.3 + 32.).clamp(36., 116.);
            if x + w > list_x + available {
                break;
            }
            if index == rollback {
                marker_x = Some(x - 4.);
                state.widgets.panel(
                    world,
                    camera,
                    "history-cursor",
                    rect(x - 4., y + 8., 1., 32.),
                    theme.accent,
                    25,
                );
            }
            if state
                .drag
                .as_ref()
                .is_some_and(|drag| drag.moved && drag.target == Some(index))
            {
                state.widgets.panel(
                    world,
                    camera,
                    "history-drop-cursor",
                    rect(x - 4., y + 5., 3., 38.),
                    theme.accent,
                    28,
                );
            }
            let mut c = control(&feature.name, None, false);
            c.selected = Some(state.selected == Some(feature.id.0));
            c.owned_keys = vec![
                KeyChord {
                    key: "ContextMenu".into(),
                    ..default()
                },
                KeyChord {
                    key: "F10".into(),
                    shift: true,
                    ..default()
                },
            ];
            let caption = feature.name.clone();
            let mut bounds = rect(x, y + 10., w, 28.);
            bounds.border = UiRect::all(px(1.));
            bounds.border_radius = BorderRadius::all(px(6.));
            let entity = state.widgets.button(
                world,
                camera,
                &format!("feature-{}", feature.id.0),
                c,
                Some(&caption),
                NativeCommand::History(HistoryCommand::Select(feature.id.0)),
                bounds,
                Some(icon(feature)),
                24,
            )?;
            if world
                .get::<interface_shell::InterfaceFlat>(entity)
                .is_some()
            {
                world
                    .entity_mut(entity)
                    .remove::<interface_shell::InterfaceFlat>();
            }
            let muted = index >= rollback || feature.suppressed;
            let ink = if matches!(feature.status, limo_cad_core::FeatureStatus::Error { .. }) {
                Color::srgb_u8(224, 85, 85)
            } else if state.selected == Some(feature.id.0) {
                theme.accent
            } else if muted {
                interface_shell::ribbon::css_mix(theme.mute, theme.panel, 0.45)
            } else {
                theme.ink
            };
            interface_shell::control_colors(world, entity, ink, theme.panel);
            interface_shell::caption_size(world, entity, 10.);
            x += w + 6.;
            shown += 1;
        }
        if count > 0 && rollback == count && state.scroll + shown == count {
            marker_x = Some(x - 2.);
            state.widgets.panel(
                world,
                camera,
                "history-cursor",
                rect(x - 2., y + 8., 1., 32.),
                theme.accent,
                25,
            );
        }
        if state
            .drag
            .as_ref()
            .is_some_and(|drag| drag.moved && drag.target == Some(state.scroll + shown))
            && shown > 0
        {
            state.widgets.panel(
                world,
                camera,
                "history-drop-cursor",
                rect(x - 3., y + 5., 3., 38.),
                theme.accent,
                28,
            );
        }
        if let Some(left) = marker_x {
            state.widgets.panel(
                world,
                camera,
                "history-cursor-head",
                rect(left - 2.5, y + 6., 6., 6.),
                theme.accent,
                26,
            );
            let mut marker = control("Rollback marker", None, locked);
            marker.owned_keys = [
                "ArrowLeft",
                "ArrowRight",
                "ArrowUp",
                "ArrowDown",
                "Home",
                "End",
            ]
            .map(KeyChord::plain)
            .into();
            let entity = state.widgets.button(
                world,
                camera,
                "history-cursor-hit",
                marker,
                Some(""),
                NativeCommand::History(HistoryCommand::RollbackMarker),
                rect(left - 6., y + 4., 13., 40.),
                None,
                27,
            )?;
            interface_shell::control_colors(world, entity, theme.accent, Color::NONE);
        }
        if state.scroll > 0 {
            state.widgets.button(
                world,
                camera,
                "scroll-back",
                control("Earlier features", None, false),
                Some("‹"),
                NativeCommand::History(HistoryCommand::Scroll(-1)),
                rect(list_x - 20., y + 12., 20., 24.),
                None,
                26,
            )?;
        }
        if state.scroll + shown < count {
            state.widgets.button(
                world,
                camera,
                "scroll-forward",
                control("Later features", None, false),
                Some("›"),
                NativeCommand::History(HistoryCommand::Scroll(1)),
                rect(width - 28., y + 12., 22., 24.),
                None,
                26,
            )?;
        }
        if let Some(target) = &state.menu {
            if let Ok(feature) = feature(&document, target.id) {
                let index = document
                    .features
                    .iter()
                    .position(|f| f.id.0 == target.id)
                    .unwrap();
                let x = target.anchor[0].min((width - 244.).max(4.));
                let y = (target.anchor[1] - 243.).max(4.);
                let scope = "history-menu";
                state.widgets.backdrop(
                    (world, camera),
                    "history-backdrop",
                    scope,
                    NativeCommand::History(HistoryCommand::Cancel),
                    rect(0., 0., width, height),
                    59,
                )?;
                let mut menu_bounds = rect(x, y, 240., 241.);
                menu_bounds.border = UiRect::all(px(1.));
                state.widgets.panel(
                    world,
                    camera,
                    "history-menu-bg",
                    menu_bounds,
                    theme.panel.with_alpha(1.),
                    60,
                );
                world
                    .entity_mut(state.widgets.entity("history-menu-bg").unwrap())
                    .insert((
                        BorderColor::all(theme.edge),
                        bevy::ui::BoxShadow::new(
                            Color::BLACK.with_alpha(0.5),
                            px(0.),
                            px(10.),
                            px(-5.),
                            px(25.),
                        ),
                    ));
                for (i, (label, command, disabled)) in [
                    (
                        if feature.kind == FeatureKind::Sketch {
                            "Edit sketch"
                        } else {
                            "Edit feature"
                        },
                        HistoryCommand::Edit(target.id),
                        locked
                            || (feature.kind != FeatureKind::Sketch
                                && super::feature::SolidFormKind::from_feature_kind(feature.kind)
                                    .is_none()),
                    ),
                    (
                        "Rename feature",
                        HistoryCommand::Rename(target.id),
                        locked || feature.kind == FeatureKind::ConstructionPlane,
                    ),
                    (
                        "Roll back before",
                        HistoryCommand::Rollback(index),
                        locked || index == rollback,
                    ),
                    (
                        "Roll forward after",
                        HistoryCommand::Rollback(index + 1),
                        locked || index + 1 == rollback,
                    ),
                    (
                        "Roll forward to end",
                        HistoryCommand::Rollback(count),
                        locked || count == rollback,
                    ),
                    (
                        "Move earlier",
                        HistoryCommand::Reorder {
                            feature_id: target.id,
                            target_index: index.saturating_sub(1),
                        },
                        locked || rollback != count || index == 0,
                    ),
                    (
                        "Move later",
                        HistoryCommand::Reorder {
                            feature_id: target.id,
                            target_index: index + 2,
                        },
                        locked || rollback != count || index + 1 >= count,
                    ),
                    ("Delete feature…", HistoryCommand::Delete(target.id), locked),
                ]
                .into_iter()
                .enumerate()
                {
                    state.widgets.button(
                        world,
                        camera,
                        &format!("history-menu-{i}"),
                        control(label, Some(scope), disabled),
                        None,
                        NativeCommand::History(command),
                        rect(x + 4., y + 4. + i as f32 * 29., 232., 28.),
                        None,
                        61,
                    )?;
                }
            }
        }
        if let Some(target) = &state.delete {
            let scope = "delete-feature";
            let w = 448_f32.min((width - 32.).max(1.));
            let x = (width - w) / 2.;
            let y = ((height - 208.) / 2.).max(0.);
            state.widgets.backdrop(
                (world, camera),
                "delete-backdrop",
                scope,
                NativeCommand::History(HistoryCommand::Cancel),
                rect(0., 0., width, height),
                69,
            )?;
            state.widgets.panel(
                world,
                camera,
                "delete-dim",
                rect(0., 0., width, height),
                Color::srgba(0., 0., 0., 0.45),
                68,
            );
            state.widgets.panel(
                world,
                camera,
                "delete-panel",
                rect(x, y, w, 208.),
                theme.panel.with_alpha(1.),
                70,
            );
            state.widgets.text(
                world,
                camera,
                "delete-title",
                rect(x + 16., y + 12., w - 32., 24.),
                "Delete feature",
                14.,
                71,
            );
            let name = feature(&document, target.id)
                .map(|f| f.name.as_str())
                .unwrap_or("this feature");
            let message=state.error.clone().unwrap_or_else(||format!("Delete {name}? Later features that depend on it may fail. You can undo this change."));
            state.widgets.text(
                world,
                camera,
                "delete-message",
                rect(x + 16., y + 51., w - 32., 93.),
                &message,
                12.,
                71,
            );
            for (key, label, command, x) in [
                (
                    "delete-cancel",
                    "Cancel",
                    HistoryCommand::Cancel,
                    x + w - 188.,
                ),
                (
                    "delete-confirm",
                    "Delete",
                    HistoryCommand::ConfirmDelete(target.id),
                    x + w - 94.,
                ),
            ] {
                let mut bounds = rect(x, y + 161., 78., 32.);
                bounds.border = UiRect::all(px(1.));
                let entity = state.widgets.button(
                    world,
                    camera,
                    key,
                    control(label, Some(scope), false),
                    None,
                    NativeCommand::History(command),
                    bounds,
                    None,
                    72,
                )?;
                if key == "delete-confirm" {
                    interface_shell::destructive_button(world, entity);
                } else if world
                    .get::<interface_shell::InterfaceFlat>(entity)
                    .is_some()
                {
                    world
                        .entity_mut(entity)
                        .remove::<interface_shell::InterfaceFlat>();
                }
            }
        }
        if let Some(rename) = &mut state.rename {
            let scope = "rename-feature";
            let w = 448_f32.min((width - 32.).max(1.));
            let x = (width - w) / 2.;
            let y = ((height - 208.) / 2.).max(0.);
            state.widgets.backdrop(
                (world, camera),
                "rename-backdrop",
                scope,
                NativeCommand::History(HistoryCommand::Cancel),
                rect(0., 0., width, height),
                69,
            )?;
            state.widgets.panel(
                world,
                camera,
                "rename-dim",
                rect(0., 0., width, height),
                Color::srgba(0., 0., 0., 0.45),
                68,
            );
            state.widgets.panel(
                world,
                camera,
                "rename-panel",
                rect(x, y, w, 208.),
                theme.panel.with_alpha(1.),
                70,
            );
            state.widgets.text(
                world,
                camera,
                "rename-title",
                rect(x + 16., y + 12., w - 32., 24.),
                "Rename feature",
                14.,
                71,
            );
            let mut field = control("Feature name", Some(scope), false);
            field.role = "textbox".into();
            field.owned_keys = vec![KeyChord::plain("Enter"), KeyChord::plain("Escape")];
            field.field = limo_cad_interface::Field::Text {
                value: rename.name.clone(),
                read_only: false,
                selection: None,
            };
            let mut bounds = rect(x + 16., y + 51., w - 32., 32.);
            bounds.border = UiRect::all(px(1.));
            bounds.padding = UiRect::axes(px(8.), px(4.));
            let entity = state.widgets.button(
                world,
                camera,
                "rename-name",
                field,
                None,
                NativeCommand::History(HistoryCommand::RenameValue(rename.target.id)),
                bounds,
                None,
                72,
            )?;
            if !rename.focus_requested {
                fields::request_focus(world, entity, owner);
                rename.focus_requested = true;
            }
            if let Some(error) = &state.error {
                state.widgets.text(
                    world,
                    camera,
                    "rename-error",
                    rect(x + 16., y + 91., w - 32., 54.),
                    error,
                    12.,
                    71,
                );
            }
            for (key, label, command, bx, disabled) in [
                (
                    "rename-cancel",
                    "Cancel",
                    HistoryCommand::Cancel,
                    x + w - 188.,
                    false,
                ),
                (
                    "rename-confirm",
                    "Rename",
                    HistoryCommand::ConfirmRename(rename.target.id),
                    x + w - 94.,
                    rename.name.trim().is_empty()
                        || rename.name.trim().chars().count() > 256
                        || rename.name.chars().any(char::is_control),
                ),
            ] {
                state.widgets.button(
                    world,
                    camera,
                    key,
                    control(label, Some(scope), disabled),
                    None,
                    NativeCommand::History(command),
                    rect(bx, y + 161., 78., 32.),
                    None,
                    72,
                )?;
            }
        }
        state.widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(state);
    result
}
