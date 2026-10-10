use super::*;
use crate::{
    app_preferences::{locale as dictionary, Locale},
    native_viewport::localization,
};
use std::sync::OnceLock;

fn catalog() -> &'static Value {
    static VALUE: OnceLock<Value> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../../../interface/catalog.json")).unwrap()
    })
}
fn label(locale: Locale, entry: &Value) -> String {
    entry["labelKey"]
        .as_str()
        .map(|key| dictionary::translate(locale, key))
        .unwrap_or_default()
        .to_owned()
}
fn key(id: &str) -> &str {
    match id {
        "fillet" => "solid-fillet",
        "chamfer" => "solid-chamfer",
        "shell" => "solid-shell",
        "externalThread" => "external-thread",
        "offsetPlane" => "offset-plane",
        "planeAtAngle" => "angle-plane",
        "mirror" => "solid-mirror",
        "patternRectangular" => "solid-rectangular-pattern",
        "patternCircular" => "solid-circular-pattern",
        "moveCopy" => "move-copy",
        "splitBody" => "split-body",
        "assemblyBrowser" => "assembly",
        "createJoint" => "joint",
        "select" => "clear",
        other => other,
    }
}
fn icon(id: &str) -> Icon {
    match id {
        "createSketch" => Icon::Sketch,
        "extrude" => Icon::Extrude,
        "revolve" => Icon::Revolve,
        "sweep" => Icon::Sweep,
        "loft" => Icon::Loft,
        "rib" => Icon::Rib,
        "hole" => Icon::Hole,
        "externalThread" => Icon::ExternalThread,
        "fillet" => Icon::Fillet,
        "chamfer" => Icon::Chamfer,
        "shell" => Icon::Shell,
        "mirror" => Icon::Mirror,
        "patternRectangular" => Icon::RectangularPattern,
        "patternCircular" => Icon::CircularPattern,
        "moveCopy" => Icon::MoveCopy,
        "combine" => Icon::Combine,
        "splitBody" => Icon::SplitBody,
        "constructionVisibility" | "offsetPlane" => Icon::OffsetPlane,
        "midplane" => Icon::Midplane,
        "planeAtAngle" => Icon::AnglePlane,
        "measure" => Icon::Measure,
        "sectionAnalysis" => Icon::Layers,
        "assemblyBrowser" => Icon::Boxes,
        "createJoint" => Icon::Joint,
        "select" => Icon::Select,
        _ => Icon::Square,
    }
}
fn workspace_icon(workspace: Workspace, light: bool) -> Icon {
    match workspace {
        Workspace::Solid => Icon::WorkspaceModel(light),
        Workspace::Drawing => Icon::FileText,
        Workspace::Cam => Icon::WorkspaceManufacture(light),
    }
}
fn source(world: &mut World, controls: &HashMap<String, Entity>, id: &str) -> Option<Entity> {
    if id == "createSketch" {
        world
            .query_filtered::<(Entity, &NativeCommandBinding), With<InterfaceControl>>()
            .iter(world)
            .find(|(_, binding)| {
                matches!(
                    &binding.command,
                    NativeCommand::Sketch(crate::native_editor::EditorCommand::Support(
                        crate::native_editor::support::Command::Start
                    ))
                )
            })
            .map(|(e, _)| e)
    } else {
        controls.get(key(id)).copied()
    }
}

/// Keep each group's primary command visible, then shed secondary commands
/// in a stable round-robin order. Every hidden command stays in its menu.
fn visible_counts(panels: &[Value], available: f32) -> Vec<usize> {
    let mut counts: Vec<_> = panels
        .iter()
        .map(|p| p["buttons"].as_array().unwrap().len())
        .collect();
    let total = |counts: &[usize]| {
        counts
            .iter()
            .map(|n| 9. + (*n as f32 * 50. - 2.).max(48.))
            .sum::<f32>()
    };
    let max = counts.iter().copied().max().unwrap_or(0);
    for slot in (1..max).rev() {
        for index in (0..counts.len()).rev() {
            if total(&counts) <= available {
                return counts;
            }
            if counts[index] > slot {
                counts[index] -= 1;
            }
        }
    }
    counts
}
fn references(
    world: &World,
    services: &NativeServices,
) -> Result<(NativeCommand, bool, bool), String> {
    let mut visibility = crate::session_bridge::parse_engine_envelope(
        services.engine.engine_call("project_visibility", ""),
    )?;
    let mut names = Vec::new();
    let mut planes = Vec::new();
    fn collect(
        nodes: &[limo_cad_core::BrowserNode],
        names: &mut Vec<String>,
        planes: &mut Vec<u64>,
    ) {
        for node in nodes {
            match node.kind {
                limo_cad_core::BrowserNodeKind::Sketch => {
                    if let Some(name) = &node.name {
                        names.push(name.clone());
                    }
                }
                limo_cad_core::BrowserNodeKind::ConstructionPlane => {
                    if let Some(id) = node.reference_id {
                        planes.push(id);
                    }
                }
                _ => {}
            }
            collect(&node.children, names, planes);
        }
    }
    services
        .engine
        .with_document(|document| collect(document.browser(), &mut names, &mut planes));
    let (_, _, view, _) = native_viewport::interface_view(world);
    let showing = names
        .iter()
        .any(|name| !view.hidden_sketch_names.contains(name))
        || planes
            .iter()
            .any(|id| !view.hidden_datum_plane_ids.contains(id));
    let disabled = names.is_empty() && planes.is_empty();
    visibility["hidden_sketch_names"] = if showing { json!(names) } else { json!([]) };
    visibility["hidden_datum_plane_ids"] = if showing { json!(planes) } else { json!([]) };
    Ok((
        NativeCommand::Mutation {
            operation: "project_set_visibility".into(),
            arguments: visibility,
        },
        disabled,
        showing,
    ))
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    controls: &HashMap<String, Entity>,
    width: f32,
    sketch: bool,
    services: &NativeServices,
    state: &mut Workbench,
) -> Result<(), String> {
    let theme = crate::native_viewport::ui::theme(world);
    let locale = localization::locale(world);
    let workspace_label = dictionary::translate(locale, workspace_label_key(state.workspace));
    let workspace_width = 108.;
    let workspace = centered_button(
        (&mut state.widgets, world, camera),
        (
            "workspace",
            dictionary::translate(locale, "workspace.switchWorkspace"),
            workspace_label,
        ),
        NativeCommand::Workbench(Command::Menu("workspace".into())),
        ribbon::node(4., 34., workspace_width - 8.),
        Some(state.menu.as_deref() == Some("workspace")),
        false,
        42,
    )?;
    let workspace_glyph = workspace_icon(state.workspace, theme.viewport.to_srgba().red > 0.7);
    ribbon::decorate(world, workspace, workspace_glyph);
    ribbon::replace_compact_glyph(world, workspace, workspace_glyph);
    let caption_right =
        ribbon::workspace_caption(world, workspace, workspace_label, workspace_width - 8.);
    state.widgets.glyph(
        (world, camera),
        "workspace-chevron",
        rect(
            (4. + caption_right + 2.).min(workspace_width - 12.),
            69.,
            8.,
            8.,
        ),
        Icon::ChevronDown,
        theme.mute,
        43,
    );
    if let Some(mut c) = world.get_mut::<InterfaceControl>(workspace) {
        c.expanded = Some(state.menu.as_deref() == Some("workspace"));
        c.modal_scope = state.menu.as_ref().map(|_| "workbench-menu".into());
    }
    state.widgets.text(
        world,
        camera,
        "workspace-title",
        rect(0., 96., workspace_width, 16.),
        dictionary::translate(locale, "ribbon.panels.workspace"),
        8.,
        30,
    );
    if let Some(e) = state.widgets.entity("workspace-title") {
        world.entity_mut(e).insert((
            TextLayout::new(Justify::Center, bevy::text::LineBreak::WordOrCharacter),
            bevy::text::LineHeight::Px(8.),
            bevy::text::LetterSpacing::Px(0.8),
            TextColor(theme.mute),
        ));
    }
    state.widgets.panel(
        world,
        camera,
        "workspace-divider",
        rect(workspace_width, 28., 1., 92.),
        theme.edge,
        30,
    );
    if sketch {
        {
            let mut badge = rect(workspace_width / 2. - 21., 82., 42., 10.);
            badge.border_radius = BorderRadius::MAX;
            state.widgets.panel(
                world,
                camera,
                "workspace-sketch-badge",
                badge,
                theme.accent_soft,
                42,
            );
            state.widgets.text(
                world,
                camera,
                "workspace-sketch",
                rect(8., 82., workspace_width - 16., 10.),
                dictionary::translate(locale, "ribbon.tabs.sketch"),
                8.,
                43,
            );
            world
                .entity_mut(state.widgets.entity("workspace-sketch").unwrap())
                .insert((
                    TextLayout::justify(Justify::Center),
                    TextColor(theme.accent),
                ));
        }
        if state.menu.as_deref() == Some("workspace") {
            menu((world, camera), width, 4., &[], controls, services, state)?;
        }
        return Ok(());
    }
    if state.workspace != Workspace::Solid {
        if let Some(entity) = source(world, controls, "createSketch") {
            world.get_mut::<InterfaceControl>(entity).unwrap().visible = false;
        }
    }
    if state.workspace == Workspace::Cam {
        cam_ribbon(world, camera, workspace_width, services, state)?;
        if state.menu.as_deref() == Some("workspace") {
            menu((world, camera), width, 4., &[], controls, services, state)?;
        }
        return Ok(());
    }
    if state.workspace == Workspace::Drawing {
        drawing_ribbon(world, camera, workspace_width, services, state)?;
        if state.menu.as_deref() == Some("workspace") {
            menu((world, camera), width, 4., &[], controls, services, state)?;
        } else if state.menu.as_deref() == Some("drawing-dimensions") {
            let entries = [
                json!({"id":"drawingChainDimensionMenu","labelKey":"ribbon.drawing.chainDimension"}),
                json!({"id":"drawingBaselineDimensionMenu","labelKey":"ribbon.drawing.baselineDimension"}),
                json!({"id":"drawingContinuedDimensionMenu","labelKey":"ribbon.drawing.continuedDimension"}),
                json!({"id":"drawingOrdinateDimensionMenu","labelKey":"ribbon.drawing.ordinateDimension"}),
                json!({"id":"drawingChamferNoteMenu","labelKey":"ribbon.drawing.chamferNote"}),
                json!({"id":"drawingHoleNoteMenu","labelKey":"ribbon.drawing.holeNote"}),
                json!({"id":"drawingCenterMarkMenu","labelKey":"ribbon.drawing.centerMark"}),
                json!({"id":"drawingCenterLineMenu","labelKey":"ribbon.drawing.centerLine"}),
                json!({"id":"drawingRevisionCloudMenu","labelKey":"ribbon.drawing.revisionCloud"}),
                json!({"id":"drawingCenterEdgesMenu","labelKey":"ribbon.drawing.centerLineBetweenEdges"}),
                json!({"id":"drawingSymmetryMenu","labelKey":"ribbon.drawing.symmetryAxis"}),
                json!({"id":"drawingBoltCircleMenu","labelKey":"ribbon.drawing.boltCircle"}),
                json!({"id":"drawingArcLengthMenu","labelKey":"ribbon.drawing.arcLength"}),
                json!({"id":"drawingJoggedRadiusMenu","labelKey":"ribbon.drawing.joggedRadius"}),
                json!({"id":"drawingDatumMenu","labelKey":"ribbon.drawing.datum"}),
                json!({"id":"drawingGdtMenu","labelKey":"ribbon.drawing.gdt"}),
                json!({"id":"drawingSurfaceMenu","labelKey":"ribbon.drawing.surfaceTexture"}),
                json!({"id":"drawingEdgeMenu","labelKey":"ribbon.drawing.edgeRequirement"}),
                json!({"id":"drawingWeldMenu","labelKey":"ribbon.drawing.weld"}),
                json!({"id":"drawingBalloonMenu","labelKey":"ribbon.drawing.balloon"}),
                json!({"id":"drawingRepairMenu","labelKey":"drawing.workspace.reassociateReferences"}),
            ];
            menu(
                (world, camera),
                width,
                workspace_width + 354.,
                &entries,
                controls,
                services,
                state,
            )?;
        }
        return Ok(());
    }
    let panels = catalog()["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == "solid")
        .unwrap()["panels"]
        .as_array()
        .unwrap();
    let counts = visible_counts(panels, width - workspace_width - 6.);
    for entity in controls.values() {
        if matches!(
            world
                .get::<NativeCommandBinding>(*entity)
                .map(|b| &b.command),
            Some(
                NativeCommand::Feature(_)
                    | NativeCommand::Assembly(_)
                    | NativeCommand::ClearSelection
            )
        ) {
            if let Some(mut c) = world.get_mut::<InterfaceControl>(*entity) {
                c.visible = false;
            }
        }
    }
    let mut x = workspace_width;
    let mut open_entries = vec![];
    for (panel, count) in panels.iter().zip(counts) {
        let id = panel["id"].as_str().unwrap();
        let buttons = panel["buttons"].as_array().unwrap();
        let group_width = 9. + (count as f32 * 50. - 2.).max(48.);
        let group_label = label(locale, panel);
        let mut entries = panel["menu"].as_array().cloned().unwrap_or_else(|| {
            buttons
                .iter()
                .map(|b| {
                    let mut b = b.clone();
                    b["type"] = json!("item");
                    b
                })
                .collect()
        });
        if id == "reference" {
            let mut item = buttons[0].clone();
            item["type"] = json!("item");
            entries.insert(0, item);
        }
        let has_menu = id != "profile" && id != "selection";
        for (i, button) in buttons.iter().enumerate() {
            let bid = button["id"].as_str().unwrap();
            let existing = source(world, controls, bid);
            if let Some(entity) = existing {
                let visible = i < count;
                let translated = label(locale, button);
                {
                    let mut control = world.get_mut::<InterfaceControl>(entity).unwrap();
                    control.visible = visible;
                    if control.label != translated {
                        control.label.clone_from(&translated);
                    }
                }
                ribbon::caption(world, entity, &translated);
                if visible {
                    world.entity_mut(entity).insert(ribbon::node(
                        x + 4. + i as f32 * 50.,
                        34.,
                        48.,
                    ));
                    if bid == "select" {
                        ribbon::decorate(world, entity, Icon::Select);
                        ribbon::caption(world, entity, &translated);
                    }
                }
            } else if i < count {
                let (command, disabled, selected) = if bid == "constructionVisibility" {
                    let (command, disabled, showing) = references(world, services)?;
                    (command, disabled, Some(showing))
                } else if bid == "sectionAnalysis" {
                    (
                        NativeCommand::SectionReview(0, section_review::Command::Open),
                        services.engine.solid_scene_snapshot().bodies.is_empty(),
                        None,
                    )
                } else {
                    (NativeCommand::Workbench(Command::Dismiss), true, None)
                };
                let entity = centered_button(
                    (&mut state.widgets, world, camera),
                    (
                        &format!("tool-{bid}"),
                        &label(locale, button),
                        &label(locale, button),
                    ),
                    command,
                    ribbon::node(x + 4. + i as f32 * 50., 34., 48.),
                    selected,
                    disabled,
                    30,
                )?;
                ribbon::decorate(world, entity, icon(bid));
                ribbon::caption(world, entity, &label(locale, button));
            }
        }
        let selected = state.menu.as_deref() == Some(id);
        let caption = group_label.clone();
        let entity = centered_button(
            (&mut state.widgets, world, camera),
            (&format!("group-{id}"), &group_label, &caption),
            NativeCommand::Workbench(Command::Menu(id.into())),
            rect(x + 4., 90., group_width - 9., 20.),
            Some(selected),
            !has_menu,
            42,
        )?;
        if let Some(mut c) = world.get_mut::<InterfaceControl>(entity) {
            c.expanded = has_menu.then_some(selected);
            c.modal_scope = state.menu.as_ref().map(|_| "workbench-menu".into());
        }
        ribbon::group_caption(
            world,
            entity,
            group_width - 9. - if has_menu { 12. } else { 0. },
        );
        if has_menu {
            let chevron_key = format!("chevron-{id}");
            state.widgets.glyph(
                (world, camera),
                &chevron_key,
                Node {
                    width: px(10.),
                    height: px(10.),
                    margin: UiRect::left(px(2.)),
                    flex_shrink: 0.,
                    ..default()
                },
                Icon::ChevronDown,
                theme.mute,
                1,
            );
            state.widgets.parent(world, &chevron_key, entity);
        }
        state.widgets.panel(
            world,
            camera,
            &format!("divider-{id}"),
            rect(x + group_width - 1., 28., 1., 92.),
            theme.edge,
            30,
        );
        if selected {
            state.menu_x = x;
            open_entries = std::mem::take(&mut entries);
        }
        x += group_width;
    }
    if state.menu.is_some() {
        menu(
            (world, camera),
            width,
            state.menu_x,
            &open_entries,
            controls,
            services,
            state,
        )?;
    }
    Ok(())
}
fn menu(
    (world, camera): (&mut World, Entity),
    width: f32,
    anchor: f32,
    entries: &[Value],
    controls: &HashMap<String, Entity>,
    services: &NativeServices,
    state: &mut Workbench,
) -> Result<(), String> {
    let theme = crate::native_viewport::ui::theme(world);
    let locale = localization::locale(world);
    let workspace = state.menu.as_deref() == Some("workspace");
    let drawing_columns = state.menu.as_deref() == Some("drawing-dimensions") && entries.len() > 12;
    let menu_width = if drawing_columns { 512. } else { 256. };
    let rows = if drawing_columns {
        entries.len().div_ceil(2)
    } else {
        entries.len()
    };
    let x = if workspace {
        4.
    } else {
        anchor.min(width - menu_width - 8.).max(4.)
    };
    let menu_height = if workspace {
        104.
    } else if drawing_columns {
        8. + rows as f32 * 30.
    } else {
        8. + entries
            .iter()
            .map(|e| if e["type"] == "separator" { 9. } else { 30. })
            .sum::<f32>()
    };
    let window_height = interface_shell::window_ui_size(world).map_or(860., |size| size.y);
    let top = 120_f32.min((window_height - menu_height - 4.).max(0.));
    state.widgets.backdrop(
        (world, camera),
        "menu-dismiss",
        "workbench-menu",
        NativeCommand::Workbench(Command::Dismiss),
        rect(0., top, width, window_height - top),
        59,
    )?;
    card(
        (&mut state.widgets, world, camera),
        "menu-card",
        rect(x, top, menu_width, menu_height),
        theme.header.with_alpha(1.),
        5.,
        60,
    );
    let e = state.widgets.entity("menu-card").unwrap();
    world.entity_mut(e).insert(bevy::ui::BoxShadow::new(
        Color::BLACK.with_alpha(0.4),
        px(0),
        px(12),
        px(0),
        px(32),
    ));
    let mut y = top + 4.;
    let workspace_entries: Vec<Value> = [Workspace::Solid, Workspace::Drawing, Workspace::Cam]
        .into_iter()
        .map(|workspace| {
            json!({
                "id": workspace_name(workspace),
                "labelKey": workspace_label_key(workspace)
            })
        })
        .collect();
    for (index, item) in if workspace {
        &workspace_entries[..]
    } else {
        entries
    }
    .iter()
    .enumerate()
    {
        let x = x + if drawing_columns && index >= rows {
            256.
        } else {
            0.
        };
        if drawing_columns && index == rows {
            y = top + 4.;
        }
        if item["type"] == "separator" {
            state.widgets.panel(
                world,
                camera,
                &format!("menu-separator-{index}"),
                rect(x + 8., y + 4., 240., 1.),
                theme.edge,
                61,
            );
            y += 9.;
            continue;
        }
        let id = item["id"].as_str().unwrap();
        let name = label(locale, item);
        let source = source(world, controls, id);
        let reference = if id == "constructionVisibility" {
            Some(references(world, services)?)
        } else {
            None
        };
        let (command, disabled) = if workspace {
            match id {
                "Solid Modeling" => (
                    NativeCommand::Workbench(Command::Workspace(Workspace::Solid)),
                    false,
                ),
                "Drawing" => (
                    NativeCommand::Workbench(Command::Workspace(Workspace::Drawing)),
                    state.sketch,
                ),
                "CAM" => (
                    NativeCommand::Workbench(Command::Workspace(Workspace::Cam)),
                    state.sketch,
                ),
                _ => (NativeCommand::Workbench(Command::Dismiss), true),
            }
        } else if let Some(tool) = series_tool(id) {
            let available = services.engine.with_drawing(|drawing| {
                drawing.sheets.iter().any(|sheet| {
                    Some(sheet.id) == drawing.active_sheet_id
                        && (tool == drawing_authoring::Tool::RevisionCloud
                            || !sheet.views.is_empty())
                })
            });
            (
                drawing_authoring::native(0, drawing_authoring::Command::Tool(tool)),
                !available,
            )
        } else if let Some(entity) = source {
            (
                world
                    .get::<NativeCommandBinding>(entity)
                    .unwrap()
                    .command
                    .clone(),
                world.get::<InterfaceControl>(entity).unwrap().disabled,
            )
        } else if let Some((command, disabled, _)) = &reference {
            (command.clone(), *disabled)
        } else if id == "sectionAnalysis" {
            (
                NativeCommand::SectionReview(0, section_review::Command::Open),
                services.engine.solid_scene_snapshot().bodies.is_empty(),
            )
        } else {
            (NativeCommand::Workbench(Command::Dismiss), true)
        };
        let mut control = InterfaceControl::button("workbench-menu", &name);
        control.modal_scope = Some("workbench-menu".into());
        control.role = "menuitem".into();
        control.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
            .map(limo_cad_interface::KeyChord::plain)
            .into();
        control.disabled = disabled;
        control.selected = reference
            .as_ref()
            .map(|(_, _, showing)| *showing)
            .or_else(|| (workspace && id == workspace_name(state.workspace)).then_some(true));
        let entity = state.widgets.button(
            world,
            camera,
            &format!("menu-row-{index}"),
            control,
            None,
            command,
            rect(x + 4., y, 248., 28.),
            Some(if workspace {
                workspace_icon(
                    match id {
                        "Drawing" => Workspace::Drawing,
                        "CAM" => Workspace::Cam,
                        _ => Workspace::Solid,
                    },
                    theme.viewport.to_srgba().red > 0.7,
                )
            } else {
                icon(id)
            }),
            62,
        )?;
        interface_shell::caption_size(world, entity, 11.);
        ribbon::menu_ink(world, entity);
        if (workspace && id == workspace_name(state.workspace))
            || reference.as_ref().is_some_and(|(_, _, showing)| *showing)
        {
            state.widgets.glyph(
                (world, camera),
                &format!("workspace-check-{index}"),
                rect(x + 230., y + 8., 12., 12.),
                Icon::Finish,
                theme.accent,
                63,
            );
        }
        y += 30.;
    }
    Ok(())
}

fn cam_ribbon(
    world: &mut World,
    camera: Entity,
    workspace_width: f32,
    _services: &NativeServices,
    state: &mut Workbench,
) -> Result<(), String> {
    cam::ribbon(world, camera, workspace_width + 4., &mut state.widgets)
}

fn workspace_name(workspace: Workspace) -> &'static str {
    match workspace {
        Workspace::Solid => "Solid Modeling",
        Workspace::Drawing => "Drawing",
        Workspace::Cam => "CAM",
    }
}

fn workspace_label_key(workspace: Workspace) -> &'static str {
    match workspace {
        Workspace::Solid => "ribbon.tabs.solidModeling",
        Workspace::Drawing => "ribbon.tabs.drawingWorkspace",
        Workspace::Cam => "ribbon.tabs.camWorkspace",
    }
}

fn drawing_ribbon(
    world: &mut World,
    camera: Entity,
    workspace_width: f32,
    services: &NativeServices,
    state: &mut Workbench,
) -> Result<(), String> {
    let locale = localization::locale(world);
    let t = |key| dictionary::translate(locale, key);
    // Capture only ribbon metadata; release the engine before updating ECS.
    let (sheet_count, active_id, active, tabs) = services.engine.with_drawing(|drawing| {
        let active_id = drawing
            .active_sheet_id
            .or_else(|| drawing.sheets.last().map(|sheet| sheet.id));
        let active = drawing
            .sheets
            .iter()
            .find(|sheet| Some(sheet.id) == active_id)
            .map(|sheet| (sheet.name.clone(), sheet.views.len()));
        let tabs: Vec<_> = drawing
            .sheets
            .iter()
            .take(6)
            .map(|sheet| (sheet.id, sheet.name.clone()))
            .collect();
        (drawing.sheets.len(), active_id, active, tabs)
    });
    let new_sheet_label = t("ribbon.drawing.newSheet");
    let new_sheet = centered_button(
        (&mut state.widgets, world, camera),
        ("drawing-new-sheet", new_sheet_label, new_sheet_label),
        NativeCommand::Mutation {
            operation: "drawing_create_sheet".into(),
            arguments: json!({
                "name": format!("Sheet {}", sheet_count + 1),
                "format": "a4",
                "orientation": "landscape"
            }),
        },
        ribbon::node(workspace_width + 4., 34., 48.),
        None,
        false,
        30,
    )?;
    ribbon::decorate(world, new_sheet, Icon::Rectangle);
    ribbon::caption(world, new_sheet, new_sheet_label);
    let delete = match active_id {
        Some(sheet_id) => NativeCommand::Mutation {
            operation: "drawing_delete_sheet".into(),
            arguments: json!({ "sheet_id": sheet_id }),
        },
        None => NativeCommand::Workbench(Command::Dismiss),
    };
    let delete_label = t("ribbon.drawing.deleteSheet");
    let delete_button = centered_button(
        (&mut state.widgets, world, camera),
        ("drawing-delete-sheet", delete_label, delete_label),
        delete,
        ribbon::node(workspace_width + 54., 34., 48.),
        None,
        active_id.is_none(),
        30,
    )?;
    ribbon::decorate(world, delete_button, Icon::Cancel);
    ribbon::caption(world, delete_button, delete_label);
    let note = drawing_authoring::native(
        0,
        drawing_authoring::Command::Tool(drawing_authoring::Tool::Note),
    );
    let note_label = t("ribbon.drawing.note");
    centered_button(
        (&mut state.widgets, world, camera),
        ("drawing-add-note", note_label, note_label),
        note,
        ribbon::node(workspace_width + 104., 34., 48.),
        None,
        active_id.is_none(),
        30,
    )?;
    let linear = t("ribbon.drawing.linearDimension");
    centered_button(
        (&mut state.widgets, world, camera),
        ("drawing-linear-dimension", linear, linear),
        drawing_authoring::native(
            0,
            drawing_authoring::Command::Tool(drawing_authoring::Tool::Linear),
        ),
        ribbon::node(workspace_width + 154., 34., 48.),
        None,
        active.as_ref().is_none_or(|(_, count)| *count == 0),
        30,
    )?;
    for (index, (key, label_key, tool)) in [
        (
            "drawing-radius",
            "ribbon.drawing.radius",
            drawing_authoring::Tool::Radial(limo_cad_sketch::DrawingRadialDimensionMode::Radius),
        ),
        (
            "drawing-diameter",
            "ribbon.drawing.diameter",
            drawing_authoring::Tool::Radial(limo_cad_sketch::DrawingRadialDimensionMode::Diameter),
        ),
        (
            "drawing-angular",
            "ribbon.drawing.angle",
            drawing_authoring::Tool::Angular,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let label = t(label_key);
        centered_button(
            (&mut state.widgets, world, camera),
            (key, label, label),
            drawing_authoring::native(0, drawing_authoring::Command::Tool(tool)),
            ribbon::node(workspace_width + 204. + index as f32 * 50., 34., 48.),
            None,
            active.as_ref().is_none_or(|(_, count)| *count == 0),
            30,
        )?;
    }
    let expanded = state.menu.as_deref() == Some("drawing-dimensions");
    let more_label = t("ribbon.drawing.moreDimensions");
    let more = centered_button(
        (&mut state.widgets, world, camera),
        ("drawing-more-dimensions", more_label, more_label),
        NativeCommand::Workbench(Command::Menu("drawing-dimensions".into())),
        ribbon::node(workspace_width + 354., 34., 48.),
        Some(expanded),
        active.is_none(),
        30,
    )?;
    if let Some(mut control) = world.get_mut::<InterfaceControl>(more) {
        control.expanded = Some(expanded);
        control.modal_scope = state.menu.as_ref().map(|_| "workbench-menu".into());
    }
    ribbon::decorate(world, more, Icon::ChevronDown);
    ribbon::caption(world, more, more_label);
    let status = match &active {
        Some((name, count)) => t("ribbon.drawing.sheetStatus")
            .replace("{name}", name)
            .replace("{count}", &count.to_string()),
        None => t("ribbon.drawing.noSheet").to_owned(),
    };
    let status_entity = centered_button(
        (&mut state.widgets, world, camera),
        ("drawing-status", &status, &status),
        NativeCommand::Workbench(Command::Dismiss),
        rect(workspace_width + 4., 90., 160., 18.),
        None,
        true,
        30,
    )?;
    sheet_caption(world, status_entity);
    for (index, (id, name)) in tabs.iter().enumerate() {
        let selected = Some(*id) == active_id;
        let tab = centered_button(
            (&mut state.widgets, world, camera),
            (&format!("drawing-sheet-{}", id), name, name),
            NativeCommand::Mutation {
                operation: "drawing_select_sheet".into(),
                arguments: json!({ "sheet_id": id }),
            },
            rect(workspace_width + 170. + index as f32 * 78., 90., 74., 18.),
            Some(selected),
            false,
            30,
        )?;
        sheet_caption(world, tab);
    }
    for (index, (key, stored_name, label_key, kind, direction, up, position)) in [
        (
            "drawing-front",
            "Front",
            "ribbon.drawing.front",
            "front",
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [110.0, 120.0],
        ),
        (
            "drawing-top",
            "Top",
            "ribbon.drawing.top",
            "top",
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0],
            [110.0, 50.0],
        ),
        (
            "drawing-bottom",
            "Bottom",
            "ribbon.drawing.bottom",
            "bottom",
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
            [110.0, 175.0],
        ),
        (
            "drawing-left",
            "Left",
            "ribbon.drawing.left",
            "left",
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [40.0, 120.0],
        ),
        (
            "drawing-right",
            "Right",
            "ribbon.drawing.right",
            "right",
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [190.0, 120.0],
        ),
        (
            "drawing-iso",
            "Isometric",
            "ribbon.drawing.isometric",
            "isometric",
            [1.0, -1.0, 1.0],
            [0.0, 0.0, 1.0],
            [230.0, 55.0],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let command = match active_id {
            Some(sheet_id) => NativeCommand::Mutation {
                operation: "drawing_add_view".into(),
                arguments: json!({
                    "sheet_id": sheet_id,
                    "view": {
                        "name": stored_name,
                        "kind": kind,
                        "direction": direction,
                        "up": up,
                        "position": position,
                        "scale": 1.0
                    }
                }),
            },
            None => NativeCommand::Workbench(Command::Dismiss),
        };
        let visible = t(label_key);
        let entity = centered_button(
            (&mut state.widgets, world, camera),
            (key, visible, visible),
            command,
            ribbon::node(workspace_width + 410. + index as f32 * 50., 34., 48.),
            None,
            active_id.is_none(),
            30,
        )?;
        ribbon::decorate(world, entity, Icon::Box);
        ribbon::caption(world, entity, visible);
    }
    Ok(())
}

/// The established sheet row is18px tall; long names stay on one line and
/// clip at its edge, while the semantic control retains the full sheet name.
fn sheet_caption(world: &mut World, entity: Entity) {
    interface_shell::compact_label(world, entity, 0.);
    interface_shell::caption_size(world, entity, 10.);
    interface_shell::caption_node(
        world,
        entity,
        Node {
            width: percent(100.),
            min_width: px(0.),
            overflow: Overflow::clip(),
            ..default()
        },
    );
    if let Some(mut node) = world.get_mut::<Node>(entity) {
        node.overflow = Overflow::clip();
    }
}

#[cfg(test)]
#[path = "ribbon_menu/localization_tests.rs"]
mod localization_tests;

fn series_tool(id: &str) -> Option<drawing_authoring::Tool> {
    use drawing_authoring::Tool;
    use limo_cad_sketch::DrawingChainDimensionLayout as Layout;
    Some(match id {
        "drawingChainDimensionMenu" => Tool::Series(Layout::Chain),
        "drawingBaselineDimensionMenu" => Tool::Series(Layout::Baseline),
        "drawingContinuedDimensionMenu" => Tool::Series(Layout::Continued),
        "drawingOrdinateDimensionMenu" => Tool::Ordinate,
        "drawingChamferNoteMenu" => Tool::Chamfer,
        "drawingHoleNoteMenu" => Tool::HoleNote,
        "drawingRevisionCloudMenu" => Tool::RevisionCloud,
        "drawingCenterMarkMenu" => Tool::CenterMark,
        "drawingCenterLineMenu" => Tool::CenterLine,
        "drawingCenterEdgesMenu" => Tool::Technical(drawing_authoring::TechnicalTool::CenterEdges),
        "drawingSymmetryMenu" => Tool::Technical(drawing_authoring::TechnicalTool::Symmetry),
        "drawingBoltCircleMenu" => Tool::Technical(drawing_authoring::TechnicalTool::BoltCircle),
        "drawingArcLengthMenu" => Tool::Technical(drawing_authoring::TechnicalTool::ArcLength),
        "drawingJoggedRadiusMenu" => {
            Tool::Technical(drawing_authoring::TechnicalTool::JoggedRadius)
        }
        "drawingDatumMenu" => Tool::Technical(drawing_authoring::TechnicalTool::Datum),
        "drawingGdtMenu" => Tool::Technical(drawing_authoring::TechnicalTool::Gdt),
        "drawingSurfaceMenu" => Tool::Technical(drawing_authoring::TechnicalTool::Surface),
        "drawingEdgeMenu" => Tool::Technical(drawing_authoring::TechnicalTool::Edge),
        "drawingWeldMenu" => Tool::Technical(drawing_authoring::TechnicalTool::Weld),
        "drawingBalloonMenu" => Tool::Technical(drawing_authoring::TechnicalTool::Balloon),
        "drawingRepairMenu" => Tool::Technical(drawing_authoring::TechnicalTool::Repair),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responsive_groups_preserve_primaries_and_restore_commands_as_space_returns() {
        let panels = catalog()["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"] == "solid")
            .unwrap()["panels"]
            .as_array()
            .unwrap();
        let compact = visible_counts(panels, 1140.);
        let roomy = visible_counts(panels, 1850.);
        assert_eq!(compact.len(), 9);
        assert!(compact.iter().all(|n| *n >= 1));
        assert!(compact.iter().sum::<usize>() < roomy.iter().sum::<usize>());
        for (panel, count) in panels.iter().zip(roomy) {
            assert_eq!(count, panel["buttons"].as_array().unwrap().len());
        }
        assert!(panels.iter().any(|p| p["menu"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["id"] == "shell"))));
        assert!(panels.iter().any(|p| p["menu"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["id"] == "planeAtAngle"))));
    }
}
