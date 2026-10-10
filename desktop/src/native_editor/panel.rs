//! Sketch groups and contextual editors use the same retained controls as
//! the rest of the application. Geometry is never edited through JSON text.
use super::*;
use crate::native_viewport::interface_shell::{self, fields, ribbon::Icon};
use crate::session_bridge::native_interface::controller::chrome::{rect, Widgets};
use limo_cad_interface::{Field, KeyChord};

#[derive(Resource, Default)]
struct Panel {
    widgets: Widgets,
    field: Option<Entity>,
    dimension_generation: Option<u64>,
    dimension_focused: bool,
    form_fields: HashMap<usize, (Entity, u64, u64)>,
    form_id: Option<u64>,
    scroll: f32,
    max_scroll: f32,
    form_area: Option<InterfaceRect>,
}

pub(super) fn retry_dimension_focus(world: &mut World) {
    if let Some(mut panel) = world.get_resource_mut::<Panel>() {
        // Modeling's busy frame disables controls and retires field focus.
        // Let the next enabled presentation focus the surviving editor again.
        panel.dimension_focused = false;
    }
}

#[derive(Clone, Copy)]
pub(super) struct RibbonGroup {
    pub key: &'static str,
    pub label: &'static str,
    pub left: f32,
    pub width: f32,
    pub count: usize,
    pub menu: bool,
}
pub(super) fn completion_reserve(tool: Option<CreateTool>) -> f32 {
    if tool == Some(CreateTool::Spline) {
        312.
    } else {
        156.
    }
}

pub(crate) fn focus_adjacent_form_field(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    current: Entity,
    backwards: bool,
) -> Result<bool, String> {
    let Some(editor) = world.get_resource::<Editor>() else {
        return Ok(false);
    };
    let Some(form) = editor.interaction.form.as_ref() else {
        return Ok(false);
    };
    let Some(stamp) = editor.stamp.as_ref().filter(|stamp| stamp.sketch.is_some()) else {
        return Ok(false);
    };
    let Some(panel) = world.get_resource::<Panel>() else {
        return Ok(false);
    };
    let Some((&index, &(_, id, binding))) = panel
        .form_fields
        .iter()
        .find(|(_, (entity, id, _))| *entity == current && *id == form.id)
    else {
        return Ok(false);
    };
    let next = if backwards {
        index.checked_sub(1)
    } else {
        index
            .checked_add(1)
            .filter(|next| *next < form.values.len())
    };
    let Some((target, target_id, target_binding)) =
        next.and_then(|next| panel.form_fields.get(&next)).copied()
    else {
        return Ok(false);
    };
    if id != target_id
        || handle.focused_key() != Some(limo_cad_interface::ControlKey(current.to_bits()))
    {
        return Ok(false);
    }
    for (entity, expected_binding) in [(current, binding), (target, target_binding)] {
        if world.get::<InterfaceControl>(entity).is_none_or(|control| {
            control.binding != expected_binding
                || !control.visible
                || control.disabled
                || !matches!(
                    control.field,
                    Field::Text {
                        read_only: false,
                        ..
                    }
                )
        }) {
            return Ok(false);
        }
    }
    let source = handle.resolve_retained(limo_cad_interface::ControlKey(current.to_bits()))?;
    if source.context != stamp.owner || source.control.binding() != binding {
        return Err("The sketch form field changed before focus could move".into());
    }
    let Ok(target) = handle.resolve_retained(limo_cad_interface::ControlKey(target.to_bits()))
    else {
        return Ok(false);
    };
    if target.context != source.context || target.control.binding() != target_binding {
        return Err("The sketch form field changed before focus could move".into());
    }
    handle.prepare_activation(&target)?;
    fields::after_window_input(world, handle)?;
    Ok(true)
}

pub(super) fn ribbon_groups(area: InterfaceRect, completion_reserve: f32) -> Vec<RibbonGroup> {
    let mut groups: Vec<_> = [
        ("draw", "DRAW", 6, true),
        ("edit", "EDIT", 5, true),
        ("dimension", "DIMENSION", 1, false),
        ("repeat", "REPEAT", 3, true),
        ("constrain", "CONSTRAIN", 5, true),
        ("selection", "SELECT", 1, false),
    ]
    .into_iter()
    .map(|(key, label, count, menu)| RibbonGroup {
        key,
        label,
        count,
        menu,
        left: 0.,
        width: 0.,
    })
    .collect();
    let available = (area.width as f32 - completion_reserve).max(0.);
    let width = |group: &RibbonGroup| {
        group.count as f32 * 50. + if group.key == "dimension" { 20. } else { 8. }
    };
    let total = |groups: &[RibbonGroup]| groups.iter().map(width).sum::<f32>();
    for slot in (1..6).rev() {
        for index in (0..groups.len()).rev() {
            if total(&groups) <= available {
                break;
            }
            if groups[index].count > slot {
                groups[index].count -= 1;
            }
        }
    }
    let mut left = area.x as f32 - 4.;
    for group in &mut groups {
        group.left = left;
        group.width = width(group);
        left += group.width;
    }
    groups
}
pub(super) fn icon(command: &EditorCommand) -> Icon {
    match command {
        EditorCommand::Support(_) | EditorCommand::Begin(_) | EditorCommand::Edit(_) => {
            Icon::Sketch
        }
        EditorCommand::Finish | EditorCommand::Complete => Icon::Finish,
        EditorCommand::Cancel => Icon::Cancel,
        EditorCommand::Palette(_) => Icon::Settings,
        EditorCommand::Interaction(command) => match command {
            InteractionCommand::Modify(ModifyTool::Trim) => Icon::Trim,
            InteractionCommand::Modify(ModifyTool::Extend) => Icon::Extend,
            InteractionCommand::Modify(ModifyTool::Break) => Icon::Break,
            InteractionCommand::Relation(relation) => Icon::Relation(relation.icon()),
            InteractionCommand::Dimension => Icon::Dimension,
            InteractionCommand::Form(kind) => match kind {
                FormKind::MoveCopy => Icon::MoveCopy,
                FormKind::Offset => Icon::Offset,
                FormKind::Fillet => Icon::Fillet,
                FormKind::Mirror => Icon::Mirror,
                FormKind::RectangularPattern => Icon::RectangularPattern,
                FormKind::CircularPattern => Icon::CircularPattern,
                FormKind::Polygon => Icon::Polygon,
                _ => Icon::Pencil,
            },
            _ => Icon::Select,
        },
        EditorCommand::Size { .. } => Icon::Dimension,
        EditorCommand::Tool(tool) => match tool {
            CreateTool::Line => Icon::Line,
            CreateTool::MidpointLine => Icon::MidpointLine,
            CreateTool::Rectangle(_) => Icon::Rectangle,
            CreateTool::Circle(_) => Icon::Circle,
            CreateTool::Arc3Point | CreateTool::ArcCenter => Icon::Arc,
            CreateTool::Slot(_) => Icon::Slot,
            CreateTool::Point => Icon::Point,
            CreateTool::Spline => Icon::Spline,
        },
    }
}

pub(super) fn group(command: &EditorCommand) -> &'static str {
    match command {
        EditorCommand::Palette(_) => "sketch/selection",
        EditorCommand::Interaction(
            InteractionCommand::Relation(_)
            | InteractionCommand::ConstraintInfo(_)
            | InteractionCommand::DeleteConstraint,
        ) => "sketch/constrain",
        EditorCommand::Interaction(
            InteractionCommand::Dimension
            | InteractionCommand::EditDimension(_)
            | InteractionCommand::DeleteDimension
            | InteractionCommand::DimensionReference
            | InteractionCommand::RepositionDimension
            | InteractionCommand::DimensionText(_)
            | InteractionCommand::ApplyDimension
            | InteractionCommand::CancelDimension,
        ) => "sketch/dimension",
        EditorCommand::Interaction(InteractionCommand::Select) => "sketch/selection",
        EditorCommand::Interaction(InteractionCommand::Form(kind)) => kind.group(),
        EditorCommand::Interaction(_) => "sketch/edit",
        _ => "sketch/draw",
    }
}
pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    editor: &mut Editor,
    area: InterfaceRect,
    canvas: InterfaceRect,
) -> Result<(), String> {
    let mut panel = world.remove_resource::<Panel>().unwrap_or_default();
    let result = (|| {
        let active = editor.stamp.as_ref().is_some_and(|s| s.sketch.is_some());
        let completion_reserve = completion_reserve(editor.draft.tool);
        panel.widgets.begin();
        let window_height = interface_shell::window_ui_size(world).map_or(860., |size| size.y);
        if active {
            for group in ribbon_groups(area, completion_reserve) {
                let RibbonGroup {
                    key,
                    label,
                    left,
                    width,
                    menu,
                    ..
                } = group;
                let mut c =
                    InterfaceControl::button(format!("sketch/{key}"), format!("{label} tools"));
                c.expanded = menu.then_some(editor.interaction.menu == Some(key));
                c.disabled = !menu;
                if !menu {
                    c.role = "heading".into();
                }
                let mut bounds = rect(left, area.y as f32 + 54., width, 18.);
                bounds.justify_content = JustifyContent::Center;
                let entity = panel.widgets.button(
                    world,
                    camera,
                    &format!("group-{key}"),
                    c,
                    Some(label),
                    NativeCommand::Sketch(EditorCommand::Interaction(InteractionCommand::Menu(
                        if editor.interaction.menu == Some(key) {
                            None
                        } else {
                            Some(key)
                        },
                    ))),
                    bounds,
                    None,
                    24,
                )?;
                interface_shell::caption_size(world, entity, 10.);
                interface_shell::center_caption(world, entity);
                interface_shell::ribbon::group_caption(
                    world,
                    entity,
                    width - if menu { 12. } else { 0. },
                );
                if menu {
                    let ink = crate::native_viewport::ui::theme(world).mute;
                    panel.widgets.glyph(
                        (world, camera),
                        &format!("chevron-{key}"),
                        Node {
                            width: px(10.),
                            height: px(10.),
                            margin: UiRect::left(px(2.)),
                            flex_shrink: 0.,
                            ..default()
                        },
                        Icon::ChevronDown,
                        ink,
                        1,
                    );
                    panel
                        .widgets
                        .parent(world, &format!("chevron-{key}"), entity);
                }
                panel.widgets.panel(
                    world,
                    camera,
                    &format!("divider-{key}"),
                    rect(left + width - 1., area.y as f32 - 6., 1., 92.),
                    crate::native_viewport::ui::theme(world).edge,
                    24,
                );
            }
            if let Some(menu) = editor.interaction.menu {
                let mut rows: Vec<(String, EditorCommand)> = match menu {
                    "draw" => [
                        CreateTool::Line,
                        CreateTool::Arc3Point,
                        CreateTool::ArcCenter,
                        CreateTool::Spline,
                        CreateTool::Rectangle(RectangleMode::TwoPoint),
                        CreateTool::Rectangle(RectangleMode::Center),
                        CreateTool::Circle(CircleMode::CenterDiameter),
                        CreateTool::Circle(CircleMode::TwoPoint),
                        CreateTool::Slot(SlotMode::CenterToCenter),
                        CreateTool::Slot(SlotMode::Overall),
                        CreateTool::Slot(SlotMode::CenterPoint),
                        CreateTool::MidpointLine,
                        CreateTool::Point,
                    ]
                    .into_iter()
                    .map(|t| (t.label().into(), EditorCommand::Tool(t)))
                    .chain(std::iter::once((
                        "Polygon".into(),
                        EditorCommand::Interaction(InteractionCommand::Form(FormKind::Polygon)),
                    )))
                    .collect(),
                    "constrain" => constraints::Relation::ALL
                        .into_iter()
                        .map(|t| {
                            (
                                t.label().into(),
                                EditorCommand::Interaction(InteractionCommand::Relation(t)),
                            )
                        })
                        .collect(),
                    "repeat" => [
                        FormKind::Mirror,
                        FormKind::RectangularPattern,
                        FormKind::CircularPattern,
                    ]
                    .into_iter()
                    .map(|kind| {
                        (
                            kind.label().into(),
                            EditorCommand::Interaction(InteractionCommand::Form(kind)),
                        )
                    })
                    .collect(),
                    _ => {
                        let mut rows = vec![
                            (
                                "Trim".into(),
                                EditorCommand::Interaction(InteractionCommand::Modify(
                                    ModifyTool::Trim,
                                )),
                            ),
                            (
                                "Extend".into(),
                                EditorCommand::Interaction(InteractionCommand::Modify(
                                    ModifyTool::Extend,
                                )),
                            ),
                            (
                                "Break".into(),
                                EditorCommand::Interaction(InteractionCommand::Modify(
                                    ModifyTool::Break,
                                )),
                            ),
                        ];
                        rows.extend(
                            [
                                FormKind::Fillet,
                                FormKind::Chamfer,
                                FormKind::Offset,
                                FormKind::MoveCopy,
                                FormKind::Scale,
                            ]
                            .into_iter()
                            .map(|kind| {
                                (
                                    kind.label().into(),
                                    EditorCommand::Interaction(InteractionCommand::Form(kind)),
                                )
                            }),
                        );
                        rows.push((
                            "Delete selected".into(),
                            EditorCommand::Interaction(InteractionCommand::Delete),
                        ));
                        rows.push((
                            "Sketch Dimension".into(),
                            EditorCommand::Interaction(InteractionCommand::Dimension),
                        ));
                        rows.push((
                            "Select".into(),
                            EditorCommand::Interaction(InteractionCommand::Select),
                        ));
                        rows
                    }
                };
                if menu == "draw" {
                    let polygon = rows.pop().unwrap();
                    rows.insert(8, polygon);
                }
                let top = area.y as f32 + 86.;
                let separator_before = |command: &EditorCommand| {
                    menu == "draw"
                        && matches!(
                            command,
                            EditorCommand::Tool(
                                CreateTool::Rectangle(RectangleMode::TwoPoint)
                                    | CreateTool::MidpointLine
                            )
                        )
                };
                let extra = rows
                    .iter()
                    .filter(|(_, command)| separator_before(command))
                    .count() as f32
                    * 9.;
                let columns = if top + rows.len() as f32 * 28. + extra + 8. > window_height {
                    2
                } else {
                    1
                };
                let rows_per_column = rows.len().div_ceil(columns).max(1);
                let menu_width = 240. * columns as f32;
                let left = ribbon_groups(area, completion_reserve)
                    .iter()
                    .find(|group| group.key == menu)
                    .map_or(area.x as f32, |group| group.left)
                    .min((area.x + area.width) as f32 - menu_width)
                    .max(0.);
                let theme = crate::native_viewport::ui::theme(world);
                panel.widgets.backdrop(
                    (world, camera),
                    "menu-backdrop",
                    "sketch-menu",
                    NativeCommand::Sketch(EditorCommand::Interaction(InteractionCommand::Menu(
                        None,
                    ))),
                    rect(0., 0., (area.x + area.width) as f32, window_height),
                    59,
                )?;
                let mut menu_bounds = rect(
                    left,
                    top,
                    menu_width,
                    rows_per_column as f32 * 28. + extra + 8.,
                );
                menu_bounds.border = UiRect::all(px(1.));
                menu_bounds.border_radius = BorderRadius::all(px(4.));
                panel.widgets.panel(
                    world,
                    camera,
                    "menu",
                    menu_bounds,
                    theme.header.with_alpha(1.),
                    60,
                );
                world
                    .entity_mut(panel.widgets.entity("menu").unwrap())
                    .insert(bevy::ui::BoxShadow::new(
                        Color::BLACK.with_alpha(0.4),
                        px(0.),
                        px(12.),
                        px(0.),
                        px(32.),
                    ));
                let mut y = top + 4.;
                for (index, (label, command)) in rows.into_iter().enumerate() {
                    let left = left + (index / rows_per_column) as f32 * 240.;
                    let row = index % rows_per_column;
                    if row == 0 {
                        y = top + 4.;
                    } else if separator_before(&command) {
                        panel.widgets.panel(
                            world,
                            camera,
                            &format!("menu-separator-{index}"),
                            rect(left + 8., y + 4., 224., 1.),
                            theme.edge,
                            61,
                        );
                        y += 9.;
                    }
                    let mut control = InterfaceControl::button(group(&command), &label);
                    control.modal_scope = Some("sketch-menu".into());
                    control.role = "menuitem".into();
                    control.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
                        .map(KeyChord::plain)
                        .into();
                    control.disabled = matches!(
                        command,
                        EditorCommand::Interaction(InteractionCommand::Delete)
                    ) && editor.interaction.selection.is_empty();
                    let glyph = icon(&command);
                    let entity = panel.widgets.button(
                        world,
                        camera,
                        &format!("menu-{menu}-{index}"),
                        control,
                        None,
                        NativeCommand::Sketch(command),
                        rect(left + 4., y, 232., 28.),
                        Some(glyph),
                        61,
                    )?;
                    interface_shell::caption_size(world, entity, 12.);
                    interface_shell::ribbon::menu_ink(world, entity);
                    y += 28.;
                }
            }
        }
        let dimension = active && editor.interaction.dimension.is_some();
        if dimension {
            let theme = crate::native_viewport::ui::theme(world);
            let inline = editor.interaction.dimension_id.is_some();
            let (left, top) = if inline {
                let stamp = editor.stamp.as_ref().ok_or("Dimension has no owner")?;
                let position = editor
                    .interaction
                    .dimension_position
                    .ok_or("Dimension has no label position")?;
                let pixel = native_viewport::interface_world_point(
                    world,
                    &stamp.owner.document_id,
                    stamp
                        .basis
                        .ok_or("Dimension has no sketch plane")?
                        .to_3d([position.x, position.y]),
                )?
                .unwrap_or([canvas.width as f32 * 0.5, canvas.height as f32 * 0.5]);
                (
                    (canvas.x as f32 + pixel[0] + 10.).clamp(
                        canvas.x as f32 + 4.,
                        (canvas.x + canvas.width) as f32 - 172.,
                    ),
                    (canvas.y as f32 + pixel[1] - 14.).clamp(
                        canvas.y as f32 + 4.,
                        (canvas.y + canvas.height) as f32 - 32.,
                    ),
                )
            } else {
                (
                    ((area.x + area.width) as f32 - 280.).max(240.),
                    area.y as f32 + 94.,
                )
            };
            if !inline {
                panel.widgets.panel(
                    world,
                    camera,
                    "dimension",
                    rect(left, top, 272., 160.),
                    theme.panel.with_alpha(1.),
                    30,
                );
                panel.widgets.text(
                    world,
                    camera,
                    "dim-title",
                    rect(left + 12., top + 8., 248., 24.),
                    "Sketch Dimension",
                    13.,
                    31,
                );
                panel.widgets.text(
                    world,
                    camera,
                    "dim-help",
                    rect(left + 12., top + 38., 248., 24.),
                    if editor.interaction.dimension_position.is_some() {
                        "Value / expression (blank measures)"
                    } else {
                        "Select geometry, then click to place"
                    },
                    11.,
                    31,
                );
            }
            let mut control = InterfaceControl::button("sketch/dimension", "Dimension value");
            control.owned_keys = vec![KeyChord::plain("Enter"), KeyChord::plain("Escape")];
            control.field = Field::Text {
                value: editor.interaction.dimension.clone().unwrap(),
                read_only: editor.interaction.dimension_reference,
                selection: None,
            };
            let mut bounds = if inline {
                rect(left, top, 128., 28.)
            } else {
                rect(left + 12., top + 68., 248., 30.)
            };
            bounds.border = UiRect::all(px(1.));
            bounds.padding = UiRect::horizontal(px(6.));
            if panel.dimension_generation != Some(editor.form_serial) {
                if let Some(field) = panel.field.take() {
                    world.despawn(field);
                }
                panel.dimension_generation = Some(editor.form_serial);
                panel.dimension_focused = false;
            }
            let field = if let Some(field) = panel.field {
                field
            } else {
                let assets = world.resource::<ViewportUiAssets>().clone();
                let field = fields::spawn_text_field(
                    &mut world.commands(),
                    camera,
                    bounds.clone(),
                    control.clone(),
                    theme,
                    &assets,
                )?;
                world.flush();
                bind_command(
                    world,
                    field,
                    NativeCommand::Sketch(EditorCommand::Interaction(
                        InteractionCommand::DimensionText(String::new()),
                    )),
                )?;
                panel.field = Some(field);
                field
            };
            let mut previous = world.get::<InterfaceControl>(field).unwrap().clone();
            previous.field = control.field;
            if world.get::<InterfaceControl>(field) != Some(&previous) {
                world.entity_mut(field).insert(previous);
            }
            if world.get::<Node>(field) != Some(&bounds) {
                world.entity_mut(field).insert(bounds);
            }
            world.entity_mut(field).insert(ZIndex(41));
            if !panel.dimension_focused && editor.interaction.dimension_position.is_some() {
                fields::request_focus(world, field, &editor.stamp.as_ref().unwrap().owner);
                panel.dimension_focused = true;
            }
            if inline {
                panel.widgets.button(
                    world,
                    camera,
                    &format!("dim-options-{}", editor.form_serial),
                    InterfaceControl::button("sketch/dimension", "Dimension actions"),
                    Some("⋯"),
                    NativeCommand::Sketch(EditorCommand::Interaction(
                        InteractionCommand::DimensionActions,
                    )),
                    rect(left + 132., top, 28., 28.),
                    None,
                    41,
                )?;
                if editor.interaction.dimension_actions {
                    panel.widgets.panel(
                        world,
                        camera,
                        "dim-menu",
                        rect(left, top + 32., 220., 100.),
                        theme.header.with_alpha(1.),
                        42,
                    );
                    for (index, (label, command)) in [
                        ("Delete Dimension", InteractionCommand::DeleteDimension),
                        (
                            "Toggle Driving / Reference",
                            InteractionCommand::DimensionReference,
                        ),
                        (
                            "Reposition Dimension",
                            InteractionCommand::RepositionDimension,
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        panel.widgets.button(
                            world,
                            camera,
                            &format!("dim-action-{}-{index}", editor.form_serial),
                            InterfaceControl::button("sketch/dimension", label),
                            None,
                            NativeCommand::Sketch(EditorCommand::Interaction(command)),
                            rect(left + 4., top + 36. + index as f32 * 30., 212., 28.),
                            None,
                            43,
                        )?;
                    }
                }
            } else {
                for (key, label, command, offset) in [
                    (
                        "dim-cancel",
                        "Cancel Dimension",
                        InteractionCommand::CancelDimension,
                        0.,
                    ),
                    (
                        "dim-apply",
                        "Apply Dimension",
                        InteractionCommand::ApplyDimension,
                        128.,
                    ),
                ] {
                    let mut control = InterfaceControl::button("sketch/dimension", label);
                    control.disabled = matches!(command, InteractionCommand::ApplyDimension)
                        && (editor.interaction.dimension_reference
                            || editor.interaction.dimension_position.is_none()
                            || editor.interaction.selection.is_empty());
                    control.selected = Some(offset != 0.);
                    let mut bounds = rect(left + 12. + offset, top + 116., 120., 30.);
                    bounds.border = UiRect::all(px(1.));
                    bounds.justify_content = JustifyContent::Center;
                    panel.widgets.button(
                        world,
                        camera,
                        key,
                        control,
                        Some(if offset == 0. { "Cancel" } else { "Apply" }),
                        NativeCommand::Sketch(EditorCommand::Interaction(command)),
                        bounds,
                        None,
                        31,
                    )?;
                    if offset != 0. {
                        interface_shell::primary_button(world, panel.widgets.entity(key).unwrap());
                    }
                }
            }
        } else {
            if let Some(field) = panel.field.take() {
                world.despawn(field);
            }
            panel.dimension_generation = None;
            panel.dimension_focused = false;
        }
        let form = editor.interaction.form.as_ref().filter(|_| active);
        if panel.form_id != form.map(|f| f.id) {
            panel.form_id = form.map(|f| f.id);
            panel.scroll = 0.;
        }
        panel.form_area = None;
        if let Some(constraint) = editor.interaction.constraint.as_ref().filter(|_| active) {
            let left = ((area.x + area.width) as f32 - 296.).max(240.);
            let top = area.y as f32 + 94.;
            let theme = crate::native_viewport::ui::theme(world);
            panel.widgets.panel(
                world,
                camera,
                "constraint",
                rect(left, top, 288., 150.),
                theme.panel.with_alpha(1.),
                30,
            );
            panel.widgets.text(
                world,
                camera,
                "constraint-title",
                rect(left + 12., top + 8., 264., 24.),
                &constraint.constraint.kind_str().replace('_', " "),
                13.,
                31,
            );
            panel.widgets.text(
                world,
                camera,
                "constraint-entities",
                rect(left + 12., top + 38., 264., 42.),
                &format!(
                    "Geometry: {}",
                    constraint
                        .constraint
                        .referenced_entities()
                        .iter()
                        .map(|id| id.0.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                11.,
                31,
            );
            panel.widgets.button(
                world,
                camera,
                "constraint-delete",
                InterfaceControl::button("sketch/constrain", "Delete Constraint"),
                None,
                NativeCommand::Sketch(EditorCommand::Interaction(
                    InteractionCommand::DeleteConstraint,
                )),
                rect(left + 12., top + 80., 264., 28.),
                None,
                31,
            )?;
            panel.widgets.button(
                world,
                camera,
                "constraint-close",
                InterfaceControl::button("sketch/constrain", "Close Constraint"),
                None,
                NativeCommand::Sketch(EditorCommand::Interaction(InteractionCommand::Select)),
                rect(left + 12., top + 114., 264., 28.),
                None,
                31,
            )?;
        }
        panel.form_fields.retain(|index, (entity, id, _)| {
            if form.is_some_and(|f| f.id == *id && *index < f.values.len()) {
                true
            } else {
                world.despawn(*entity);
                false
            }
        });
        if let Some(form) = form {
            let theme = crate::native_viewport::ui::theme(world);
            let assets = world.resource::<ViewportUiAssets>().clone();
            let left = ((area.x + area.width) as f32 - 296.).max(240.);
            let top = area.y as f32 + 94.;
            let height =
                (168. + form.values.len() as f32 * 52.).min((window_height - top - 84.).max(220.));
            let body_height = height - 160.;
            panel.max_scroll = (form.values.len() as f32 * 52. - body_height).max(0.);
            panel.scroll = panel.scroll.clamp(0., panel.max_scroll);
            panel.form_area = Some(InterfaceRect {
                x: f64::from(left),
                y: f64::from(top),
                width: 288.,
                height: f64::from(height),
            });
            panel.widgets.panel(
                world,
                camera,
                "form",
                rect(left, top, 288., height),
                theme.panel.with_alpha(1.),
                30,
            );
            panel.widgets.text(
                world,
                camera,
                "form-title",
                rect(left + 12., top + 8., 264., 24.),
                form.kind.label(),
                13.,
                31,
            );
            panel.widgets.text(
                world,
                camera,
                "form-instruction",
                rect(left + 12., top + 34., 264., 42.),
                form.kind.instruction(),
                11.,
                31,
            );
            panel.widgets.panel(
                world,
                camera,
                "form-body",
                rect(left + 12., top + 78., 264., body_height),
                Color::NONE,
                31,
            );
            let body = panel.widgets.entity("form-body").unwrap();
            let mut content_bounds = rect(0., -panel.scroll, 264., form.values.len() as f32 * 52.);
            content_bounds.overflow = Overflow::visible();
            panel.widgets.panel(
                world,
                camera,
                "form-content",
                content_bounds,
                Color::NONE,
                31,
            );
            panel.widgets.parent(world, "form-content", body);
            let content = panel.widgets.entity("form-content").unwrap();
            for (index, ((label, _), value)) in
                form.kind.fields().iter().zip(&form.values).enumerate()
            {
                let y = index as f32 * 52.;
                panel.widgets.text(
                    world,
                    camera,
                    &format!("form-label-{index}"),
                    rect(0., y, 264., 18.),
                    label,
                    11.,
                    31,
                );
                panel
                    .widgets
                    .parent(world, &format!("form-label-{index}"), content);
                let mut control = InterfaceControl::button(form.kind.group(), *label);
                control.owned_keys = vec![KeyChord::plain("Enter"), KeyChord::plain("Escape")];
                control.field = Field::Text {
                    value: value.clone(),
                    read_only: false,
                    selection: None,
                };
                let mut bounds = rect(0., y + 18., 264., 30.);
                bounds.border = UiRect::all(px(1.));
                bounds.padding = UiRect::horizontal(px(6.));
                let entity = if let Some((entity, _, _)) = panel.form_fields.get(&index) {
                    *entity
                } else {
                    let entity = fields::spawn_text_field(
                        &mut world.commands(),
                        camera,
                        bounds.clone(),
                        control.clone(),
                        theme,
                        &assets,
                    )?;
                    world.flush();
                    bind_command(
                        world,
                        entity,
                        NativeCommand::Sketch(EditorCommand::Interaction(
                            InteractionCommand::FormValue {
                                id: form.id,
                                index,
                                text: String::new(),
                            },
                        )),
                    )?;
                    let binding = world.get::<InterfaceControl>(entity).unwrap().binding;
                    panel.form_fields.insert(index, (entity, form.id, binding));
                    entity
                };
                let mut old = world.get::<InterfaceControl>(entity).unwrap().clone();
                old.field = control.field;
                if world.get::<InterfaceControl>(entity) != Some(&old) {
                    world.entity_mut(entity).insert(old);
                }
                if world.get::<Node>(entity) != Some(&bounds) {
                    world.entity_mut(entity).insert(bounds);
                }
                world.entity_mut(entity).insert(ZIndex(31));
                if world.get::<ChildOf>(entity).map(ChildOf::parent) != Some(content) {
                    world.entity_mut(entity).insert(ChildOf(content));
                }
            }
            let y = top + height - 82.;
            if matches!(form.kind, FormKind::MoveCopy | FormKind::Polygon) {
                let label = if form.kind == FormKind::MoveCopy {
                    "Create copy"
                } else {
                    "Circumscribed"
                };
                let mut c = InterfaceControl::button(form.kind.group(), label);
                c.field = Field::Toggle(form.option);
                c.selected = Some(form.option);
                c.role = "checkbox".into();
                let option = panel.widgets.button(
                    world,
                    camera,
                    "form-option",
                    c,
                    None,
                    NativeCommand::Sketch(EditorCommand::Interaction(
                        InteractionCommand::FormOption { id: form.id },
                    )),
                    rect(left + 12., y, 264., 28.),
                    None,
                    31,
                )?;
                interface_shell::checkbox_button(world, option, camera, form.option);
            } else {
                panel.widgets.text(
                    world,
                    camera,
                    "form-selection",
                    rect(left + 12., y, 264., 28.),
                    &format!("{} selected", editor.interaction.selection.len()),
                    11.,
                    31,
                );
            }
            for (key, label, command, offset) in [
                (
                    "form-cancel",
                    "Cancel",
                    InteractionCommand::CancelForm { id: form.id },
                    0.,
                ),
                (
                    "form-apply",
                    "Apply",
                    InteractionCommand::ApplyForm { id: form.id },
                    136.,
                ),
            ] {
                let mut c = InterfaceControl::button(
                    form.kind.group(),
                    format!("{label} {}", form.kind.label()),
                );
                c.selected = Some(offset != 0.);
                c.disabled = offset != 0.
                    && form.kind != FormKind::Polygon
                    && editor.interaction.selection.is_empty();
                let mut bounds = rect(left + 12. + offset, y + 40., 128., 30.);
                bounds.border = UiRect::all(px(1.));
                bounds.justify_content = JustifyContent::Center;
                panel.widgets.button(
                    world,
                    camera,
                    key,
                    c,
                    Some(label),
                    NativeCommand::Sketch(EditorCommand::Interaction(command)),
                    bounds,
                    None,
                    31,
                )?;
                if offset != 0. {
                    interface_shell::primary_button(world, panel.widgets.entity(key).unwrap());
                }
            }
        }
        panel.widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(panel);
    result
}

pub(crate) fn modal(world: &World) -> Option<&'static str> {
    world
        .get_resource::<Editor>()
        .and_then(|e| e.interaction.menu)
        .map(|_| "sketch-menu")
}
pub(crate) fn scroll_panel(world: &mut World, point: [f32; 2], delta: f32) -> bool {
    if super::palette::scroll(world, point, delta) {
        return true;
    }
    let Some(mut panel) = world.get_resource_mut::<Panel>() else {
        return false;
    };
    let Some(a) = panel.form_area else {
        return false;
    };
    if !delta.is_finite()
        || !point.iter().all(|v| v.is_finite())
        || f64::from(point[0]) < a.x
        || f64::from(point[0]) > a.x + a.width
        || f64::from(point[1]) < a.y
        || f64::from(point[1]) > a.y + a.height
    {
        return false;
    }
    panel.scroll = (panel.scroll - delta).clamp(0., panel.max_scroll);
    true
}
pub(crate) fn escape(world: &mut World) {
    if let Some(mut editor) = world.get_resource_mut::<Editor>() {
        editor.interaction.menu = None;
    }
}

#[cfg(test)]
mod scaled_tests {
    use super::*;

    #[test]
    fn every_sketch_menu_item_fits_at_minimum_window_and_largest_interface_size() {
        let mut world = World::new();
        world.init_resource::<ViewportUiAssets>();
        world.insert_resource(bevy::ui::UiScale(1.75));
        world.spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(2400, 1520)
                    .with_scale_factor_override(2.),
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        let camera = world.spawn_empty().id();
        let mut editor = Editor {
            stamp: Some(Stamp {
                owner: DocumentContext {
                    window_id: "main".into(),
                    document_id: "sketch".into(),
                    epoch: 1,
                },
                revision: 1,
                sketch: Some("Sketch1".into()),
                basis: None,
            }),
            ..default()
        };
        let width = 1200. / 1.75;
        let height = 760. / 1.75;
        let pixels = |value| match value {
            Val::Px(value) => value,
            _ => panic!("Expected UI pixels"),
        };
        for menu in ["draw", "edit", "repeat", "constrain"] {
            editor.interaction.menu = Some(menu);
            synchronize(
                &mut world,
                camera,
                &mut editor,
                InterfaceRect {
                    x: 64.,
                    y: 34.,
                    width: f64::from(width - 76.),
                    height: 72.,
                },
                InterfaceRect {
                    x: 64.,
                    y: 120.,
                    width: f64::from(width - 64.),
                    height: f64::from(height - 168.),
                },
            )
            .unwrap();
            let mut count = 0;
            for (control, node) in world.query::<(&InterfaceControl, &Node)>().iter(&world) {
                if control.role != "menuitem" {
                    continue;
                }
                count += 1;
                assert!(
                    pixels(node.left) >= 0. && pixels(node.top) >= 0.,
                    "{menu}: {}",
                    control.label
                );
                assert!(
                    pixels(node.left) + pixels(node.width) <= width + 0.01,
                    "{menu}: {}",
                    control.label
                );
                assert!(
                    pixels(node.top) + pixels(node.height) <= height + 0.01,
                    "{menu}: {}",
                    control.label
                );
            }
            assert!(count > 0, "{menu}");
        }
    }
}
