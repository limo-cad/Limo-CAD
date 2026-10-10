//! Sketch visibility is presentation state; snap and dimension style remain
//! authoritative document settings. The palette uses the shared control path.
use super::*;
use crate::native_viewport::interface_shell::{self, ribbon::Icon};
use crate::session_bridge::native_interface::controller::chrome::{rect, Widgets};
use limo_cad_core::DimensionStyle;
use limo_cad_interface::Field;

pub(super) const WIDTH: f32 = 240.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteCommand {
    Collapse,
    LookAt,
    Grid,
    Snap,
    Points,
    Dimensions,
    Constraints,
    Iso,
    Scroll(i8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Visibility {
    pub hide_grid: bool,
    pub hide_points: bool,
    pub hide_dimensions: bool,
    pub hide_constraints: bool,
}

#[derive(Resource, Default)]
struct Palette {
    widgets: Widgets,
    collapsed: bool,
    visibility: Visibility,
    stamp: Option<Stamp>,
    sketch: Option<SketchDto>,
    scroll: f32,
    max_scroll: f32,
    area: Option<InterfaceRect>,
}

pub(super) fn visibility(world: &World) -> Visibility {
    world
        .get_resource::<Palette>()
        .map(|p| p.visibility)
        .unwrap_or_default()
}

pub(super) fn execute(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    editor: &mut Editor,
    command: PaletteCommand,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    bridge.with_native_document_owner(engine, owner, validate)?;
    let sketch = active(engine)?.ok_or("Start or edit a sketch first")?;
    world.init_resource::<Palette>();
    match command {
        PaletteCommand::Scroll(direction) => {
            let mut state = world.resource_mut::<Palette>();
            state.scroll = (state.scroll + f32::from(direction) * 120.).clamp(0., state.max_scroll);
        }
        PaletteCommand::LookAt => look_at_sketch(world, engine, bridge, owner, false)?,
        PaletteCommand::Snap | PaletteCommand::Iso => {
            let (operation, arguments) = match command {
                PaletteCommand::Snap => {
                    ("sketch_set_grid_snap", json!({"enabled":!sketch.grid_snap}))
                }
                _ => (
                    "sketch_set_dimension_style",
                    json!({"style": if sketch.dimension_style == DimensionStyle::Iso { "aligned" } else { "iso" }}),
                ),
            };
            return queue_mutation(
                world,
                editor.stamp.as_ref().ok_or("No active sketch")?.clone(),
                operation,
                arguments,
                Completion::Settings,
            );
        }
        command => {
            let mut state = world.resource_mut::<Palette>();
            let value = match command {
                PaletteCommand::Collapse => &mut state.collapsed,
                PaletteCommand::Grid => &mut state.visibility.hide_grid,
                PaletteCommand::Points => &mut state.visibility.hide_points,
                PaletteCommand::Dimensions => &mut state.visibility.hide_dimensions,
                PaletteCommand::Constraints => &mut state.visibility.hide_constraints,
                _ => unreachable!(),
            };
            *value = !*value;
        }
    }
    Ok(json!({"palette_updated":true}))
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    editor: &Editor,
    canvas: InterfaceRect,
) -> Result<(), String> {
    let mut state = world.remove_resource::<Palette>().unwrap_or_default();
    let result = (|| {
        if state.stamp != editor.stamp {
            state.sketch = active(&services.engine)?;
            state.stamp = editor.stamp.clone();
        }
        let (_, _, mut presentation, _) = native_viewport::interface_view_snapshot(world);
        if presentation.hide_sketch_grid != state.visibility.hide_grid
            || presentation.hide_sketch_points != state.visibility.hide_points
        {
            presentation.hide_sketch_grid = state.visibility.hide_grid;
            presentation.hide_sketch_points = state.visibility.hide_points;
            native_viewport::apply_interface_view(
                world,
                &owner.document_id,
                None,
                Some(presentation),
            )?;
        }
        state.widgets.begin();
        state.area = None;
        if let Some(sketch) = state.sketch.as_ref().filter(|_| {
            editor.interaction.form.is_none()
                && editor.interaction.dimension.is_none()
                && editor.interaction.constraint.is_none()
        }) {
            let x = (canvas.x + canvas.width - f64::from(WIDTH)) as f32;
            let y = canvas.y as f32;
            let theme = crate::native_viewport::ui::theme(world);
            let expanded_height = 427_f32.min(canvas.height as f32);
            let height = if state.collapsed {
                32.
            } else {
                expanded_height
            };
            let mut bounds = rect(x, y, WIDTH, height);
            bounds.border_radius = BorderRadius::ZERO;
            bounds.border = UiRect::left(px(1.));
            state.widgets.panel(
                world,
                camera,
                "palette",
                bounds,
                theme.panel.with_alpha(1.),
                24,
            );
            let mut title = InterfaceControl::button("sketch/selection", "Sketch Palette");
            title.expanded = Some(!state.collapsed);
            let e = state.widgets.button(
                world,
                camera,
                "title",
                title,
                Some("SKETCH PALETTE"),
                NativeCommand::Sketch(EditorCommand::Palette(PaletteCommand::Collapse)),
                rect(x, y, WIDTH, 32.),
                Some(if state.collapsed {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                }),
                25,
            )?;
            interface_shell::caption_size(world, e, 10.);
            interface_shell::caption_tracking(world, e, 1.);
            interface_shell::caption_weight(world, e, FontWeight::SEMIBOLD);
            state.widgets.panel(
                world,
                camera,
                "title-divider",
                rect(x, y + 31., WIDTH, 1.),
                theme.edge,
                26,
            );
            if !state.collapsed {
                state.widgets.text(
                    world,
                    camera,
                    "options",
                    rect(x + 12., y + 38., 216., 18.),
                    "Options",
                    10.,
                    25,
                );
                world
                    .entity_mut(state.widgets.entity("options").unwrap())
                    .insert((TextColor(theme.mute), bevy::text::LetterSpacing::Px(0.5)));
                let body_height = (height - 103.).max(0.);
                state.max_scroll = (324. - body_height).max(0.);
                state.scroll = state.scroll.clamp(0., state.max_scroll);
                state.area = Some(InterfaceRect {
                    x: f64::from(x),
                    y: f64::from(y),
                    width: f64::from(WIDTH),
                    height: f64::from(height),
                });
                state.widgets.panel(
                    world,
                    camera,
                    "body",
                    rect(x, y + 59., WIDTH, body_height),
                    Color::NONE,
                    25,
                );
                let body = state.widgets.entity("body").unwrap();
                let mut content_bounds = rect(0., -state.scroll, WIDTH, 324.);
                content_bounds.overflow = Overflow::visible();
                state
                    .widgets
                    .panel(world, camera, "content", content_bounds, Color::NONE, 25);
                state.widgets.parent(world, "content", body);
                let content = state.widgets.entity("content").unwrap();
                let rows = [
                    ("Linetype", None, false),
                    ("Return to Flat View", Some(PaletteCommand::LookAt), false),
                    (
                        "Sketch Grid",
                        Some(PaletteCommand::Grid),
                        !state.visibility.hide_grid,
                    ),
                    ("Snap", Some(PaletteCommand::Snap), sketch.grid_snap),
                    ("Slice", None, false),
                    ("Profile", None, false),
                    (
                        "Points",
                        Some(PaletteCommand::Points),
                        !state.visibility.hide_points,
                    ),
                    (
                        "Dimensions",
                        Some(PaletteCommand::Dimensions),
                        !state.visibility.hide_dimensions,
                    ),
                    (
                        "Constraints",
                        Some(PaletteCommand::Constraints),
                        !state.visibility.hide_constraints,
                    ),
                    ("Projected Geometries", None, false),
                    ("Construction Geometries", None, false),
                    ("3D Sketch", None, false),
                    (
                        "ISO Dimension Style",
                        Some(PaletteCommand::Iso),
                        sketch.dimension_style == DimensionStyle::Iso,
                    ),
                ];
                for (i, (label, command, checked)) in rows.into_iter().enumerate() {
                    let top = i as f32 * 24. + if i > 1 { 12. } else { 0. };
                    let mut control = InterfaceControl::button("sketch/selection", label);
                    control.disabled = command.is_none();
                    if command != Some(PaletteCommand::LookAt) {
                        control.role = "checkbox".into();
                        control.field = Field::Toggle(checked);
                    }
                    let key = format!("row-{i}");
                    let mut bounds = rect(4., top, 232., 24.);
                    if command == Some(PaletteCommand::LookAt) {
                        bounds = rect(8., top + 4., 224., 28.);
                        bounds.border = UiRect::all(px(1.));
                    }
                    let entity = state.widgets.button(
                        world,
                        camera,
                        &key,
                        control,
                        None,
                        NativeCommand::Sketch(EditorCommand::Palette(
                            command.unwrap_or(PaletteCommand::Collapse),
                        )),
                        bounds,
                        if command == Some(PaletteCommand::LookAt) {
                            Some(Icon::Focus)
                        } else {
                            None
                        },
                        25,
                    )?;
                    state.widgets.parent(world, &key, content);
                    interface_shell::caption_size(world, entity, 12.);
                    if command == Some(PaletteCommand::LookAt) {
                        interface_shell::control_colors(world, entity, theme.ink, theme.header);
                        state.widgets.text(
                            world,
                            camera,
                            "look-arrow",
                            rect(202., 2., 16., 24.),
                            "›",
                            16.,
                            26,
                        );
                        state.widgets.parent(world, "look-arrow", entity);
                    } else {
                        let box_key = format!("box-{i}");
                        let mut bounds = rect(208., 5., 14., 14.);
                        bounds.border = UiRect::all(px(1.));
                        bounds.border_radius = BorderRadius::all(px(2.));
                        state.widgets.panel(
                            world,
                            camera,
                            &box_key,
                            bounds,
                            if checked { theme.accent } else { Color::NONE },
                            26,
                        );
                        state.widgets.parent(world, &box_key, entity);
                        let box_entity = state.widgets.entity(&box_key).unwrap();
                        let border = BorderColor::all(if checked {
                            theme.accent
                        } else {
                            theme
                                .mute
                                .with_alpha(if command.is_some() { 0.6 } else { 0.2 })
                        });
                        if world.get::<BorderColor>(box_entity) != Some(&border) {
                            world.entity_mut(box_entity).insert(border);
                        }
                        if checked {
                            let check_key = format!("check-{i}");
                            state.widgets.glyph(
                                (world, camera),
                                &check_key,
                                rect(1., 1., 10., 10.),
                                Icon::Finish,
                                Color::WHITE,
                                27,
                            );
                            state.widgets.parent(world, &check_key, box_entity);
                        }
                    }
                }
                if state.max_scroll > 0. {
                    for (key, label, direction, offset) in [
                        ("previous", "Previous options", -1, 8.),
                        ("next", "More options", 1, 124.),
                    ] {
                        let mut c = InterfaceControl::button("sketch/selection", label);
                        c.disabled = if direction < 0 {
                            state.scroll <= 0.
                        } else {
                            state.scroll >= state.max_scroll
                        };
                        state.widgets.button(
                            world,
                            camera,
                            key,
                            c,
                            None,
                            NativeCommand::Sketch(EditorCommand::Palette(PaletteCommand::Scroll(
                                direction,
                            ))),
                            rect(x + offset, y + height - 56., 108., 20.),
                            None,
                            25,
                        )?;
                    }
                }
                let mut bounds = rect(x + 8., y + height - 36., 224., 28.);
                bounds.justify_content = JustifyContent::Center;
                bounds.border = UiRect::all(px(1.));
                let e = state.widgets.button(
                    world,
                    camera,
                    "finish",
                    InterfaceControl::button("sketch/draw", "Finish Sketch from palette"),
                    Some("Finish Sketch"),
                    NativeCommand::Sketch(EditorCommand::Finish),
                    bounds,
                    None,
                    25,
                )?;
                interface_shell::caption_size(world, e, 12.);
                interface_shell::control_colors(world, e, theme.ink, theme.header);
            }
        }
        state.widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(state);
    result
}

pub(super) fn scroll(world: &mut World, point: [f32; 2], delta: f32) -> bool {
    let Some(mut state) = world.get_resource_mut::<Palette>() else {
        return false;
    };
    let Some(a) = state.area else {
        return false;
    };
    if !delta.is_finite()
        || !point.iter().all(|v| v.is_finite())
        || f64::from(point[0]) < a.x
        || f64::from(point[0]) >= a.x + a.width
        || f64::from(point[1]) < a.y
        || f64::from(point[1]) >= a.y + a.height
    {
        return false;
    }
    state.scroll = (state.scroll - delta).clamp(0., state.max_scroll);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_scrolling_stays_within_its_visible_surface_and_content() {
        let mut world = World::new();
        world.insert_resource(Palette {
            area: Some(InterfaceRect {
                x: 100.,
                y: 120.,
                width: f64::from(WIDTH),
                height: 260.,
            }),
            max_scroll: 160.,
            ..default()
        });
        assert!(!scroll(&mut world, [99., 200.], -100.));
        assert!(!scroll(&mut world, [110., 200.], f32::NAN));
        assert!(scroll(&mut world, [110., 200.], -300.));
        assert_eq!(world.resource::<Palette>().scroll, 160.);
        assert!(scroll(&mut world, [110., 200.], 300.));
        assert_eq!(world.resource::<Palette>().scroll, 0.);
        world.resource_mut::<Palette>().area = None;
        assert!(!scroll(&mut world, [110., 200.], -100.));
    }

    #[test]
    fn snap_snapshot_reports_the_engine_setting_without_changing_geometry() {
        let engine = AppState::new();
        let call = |method: &str, args: Value| {
            let result: Value =
                serde_json::from_str(&engine.engine_call(method, &args.to_string())).unwrap();
            assert_eq!(result["ok"], true, "{result}");
            result["value"].clone()
        };
        call("begin_sketch", json!({"type":"origin_plane","plane":"xy"}));
        call(
            "add_rectangle",
            json!({"mode":"two_point","p1":{"x":10.,"y":10.},"p2":{"x":40.,"y":30.},"ctrl_held":false}),
        );
        let original = active(&engine).unwrap().unwrap();
        assert!(original.grid_snap);
        for enabled in [false, true] {
            call("set_grid_snap", json!({"enabled":enabled}));
            let current = active(&engine).unwrap().unwrap();
            assert_eq!(current.grid_snap, enabled);
            assert_eq!(current.entities, original.entities);
            assert_eq!(current.constraints, original.constraints);
        }
        let mut legacy = serde_json::to_value(&original).unwrap();
        legacy.as_object_mut().unwrap().remove("grid_snap");
        assert!(
            serde_json::from_value::<SketchDto>(legacy)
                .unwrap()
                .grid_snap
        );
    }
}
