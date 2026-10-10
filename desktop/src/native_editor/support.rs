//! Create Sketch support selection. The picked persistent reference is sent
//! to the existing sketch command; hovering and choosing an origin are drafts.
use super::*;
use crate::native_viewport::interface_shell::{self, ribbon::Icon};
use crate::native_viewport::{ViewportMode, ViewportOriginPlane};
use crate::session_bridge::native_interface::controller::chrome::{rect, Widgets};
use limo_cad_core::{FaceId, OriginPlane};
use limo_cad_sketch::FaceSketchOrigin;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    Start,
    Origin(FaceSketchOrigin),
    Confirm,
}
#[derive(Default)]
pub(super) struct Picker {
    pub active: bool,
    hovered: Option<PlaneRef>,
    face: Option<FaceId>,
    origin: FaceSketchOrigin,
}
#[derive(Resource, Default)]
struct Panel(Widgets);

pub(crate) fn picking(world: &World) -> bool {
    world
        .get_resource::<Editor>()
        .is_some_and(|e| e.support.active)
}
pub(crate) fn modal(world: &World) -> Option<&'static str> {
    world
        .get_resource::<Editor>()?
        .support
        .face
        .map(|_| "sketch-origin")
}
pub(crate) fn cancel(world: &mut World, owner: &DocumentContext) -> Result<(), String> {
    if let Some(mut editor) = world.get_resource_mut::<Editor>() {
        editor.support = Default::default();
    }
    present(world, owner, &Picker::default())
}

pub(super) fn execute(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    editor: &mut Editor,
    command: Command,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    bridge.with_native_document_owner(engine, owner, validate)?;
    if editor.stamp.as_ref().is_some_and(|s| s.sketch.is_some()) {
        return Err("Finish the current sketch before starting another".into());
    }
    if crate::session_bridge::native_interface::feature::panel(world).is_some() {
        return Err("Finish or cancel the feature before creating a sketch".into());
    }
    match command {
        Command::Start => {
            let (_, _, view, _) = native_viewport::interface_view(world);
            let face = view
                .selected_face_ids
                .first()
                .filter(|_| view.selected_face_ids.len() == 1)
                .and_then(|id| {
                    engine
                        .solid_scene_snapshot()
                        .bodies
                        .iter()
                        .flat_map(|b| &b.faces)
                        .find(|f| f.id.0 == *id && f.plane.is_some())
                        .map(|f| f.id)
                });
            editor.support = Picker {
                active: true,
                face,
                origin: FaceSketchOrigin::FaceCenter,
                ..default()
            };
            editor.error.clear();
            present(world, owner, &editor.support)?;
            Ok(json!({"picking_support":true}))
        }
        Command::Origin(origin) => {
            if editor.support.face.is_none() {
                return Err("Select a planar face first".into());
            }
            if origin == FaceSketchOrigin::SupportOrigin {
                return Err("Choose the face center or projected document origin".into());
            }
            editor.support.origin = origin;
            Ok(json!({"face_origin":origin}))
        }
        Command::Confirm => {
            let face_id = editor.support.face.ok_or("Select a planar face first")?;
            queue_mutation(
                world,
                editor
                    .stamp
                    .as_ref()
                    .ok_or("Sketch support has no owner")?
                    .clone(),
                "sketch_begin",
                json!({"plane":PlaneRef::PlanarFace {face_id},"face_origin":editor.support.origin}),
                Completion::Begin,
            )
        }
    }
}

pub(super) fn pick(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    editor: &mut Editor,
    point: [f32; 2],
) -> Result<Value, String> {
    let reference =
        services
            .bridge
            .with_native_document_receipt(&services.engine, owner, |revision| {
                if editor
                    .stamp
                    .as_ref()
                    .is_none_or(|s| s.owner != *owner || s.revision != revision)
                {
                    return Err("The model changed; select the sketch support again".into());
                }
                native_viewport::interface_support_pick(world, &owner.document_id, point)
            })?;
    match reference {
        Some(PlaneRef::PlanarFace { face_id }) => {
            editor.support.face = Some(face_id);
            editor.support.origin = FaceSketchOrigin::FaceCenter;
            editor.support.hovered = None;
            present(world, owner, &editor.support)?;
            Ok(json!({"selected_face":face_id,"choose_origin":true}))
        }
        Some(plane) => queue_mutation(
            world,
            editor.stamp.as_ref().unwrap().clone(),
            "sketch_begin",
            json!({"plane":plane}),
            Completion::Begin,
        ),
        None => Ok(json!({"handled":true,"picking_support":true})),
    }
}
pub(super) fn hover(
    world: &mut World,
    owner: &DocumentContext,
    picker: &mut Picker,
    point: Option<[f32; 2]>,
) -> Result<(), String> {
    if !picker.active || picker.face.is_some() {
        return Ok(());
    }
    picker.hovered = point
        .map(|point| native_viewport::interface_support_pick(world, &owner.document_id, point))
        .transpose()?
        .flatten();
    present(world, owner, picker)
}
fn origin(plane: OriginPlane) -> ViewportOriginPlane {
    match plane {
        OriginPlane::Xy => ViewportOriginPlane::Xy,
        OriginPlane::Xz => ViewportOriginPlane::Xz,
        OriginPlane::Yz => ViewportOriginPlane::Yz,
    }
}
pub(super) fn present(
    world: &mut World,
    owner: &DocumentContext,
    picker: &Picker,
) -> Result<(), String> {
    let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    let old = view.clone();
    if picker.active {
        view.mode = ViewportMode::PickPlane;
        view.hovered_origin_plane = None;
        view.hovered_datum_plane_id = None;
        view.hovered_face_id = None;
        match picker.hovered {
            Some(PlaneRef::OriginPlane { plane }) => {
                view.hovered_origin_plane = Some(origin(plane))
            }
            Some(PlaneRef::DatumPlane { datum_id }) => {
                view.hovered_datum_plane_id = Some(datum_id.0)
            }
            Some(PlaneRef::PlanarFace { face_id }) => view.hovered_face_id = Some(face_id.0),
            None => {}
        }
        if let Some(face) = picker.face {
            view.selected_face_ids = vec![face.0];
        }
    } else if view.mode == ViewportMode::PickPlane {
        view.mode = ViewportMode::Solid;
        view.hovered_origin_plane = None;
        view.hovered_datum_plane_id = None;
        view.hovered_face_id = None;
    }
    if view != old {
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
    }
    Ok(())
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    editor: &Editor,
    canvas: InterfaceRect,
) -> Result<(), String> {
    let mut panel = world.remove_resource::<Panel>().unwrap_or_default();
    let result = (|| {
        let widgets = &mut panel.0;
        widgets.begin();
        let theme = crate::native_viewport::ui::theme(world);
        let picker = &editor.support;
        if picker.active && picker.face.is_none() {
            let x = (canvas.x + canvas.width / 2. - 170.) as f32;
            let y = canvas.y as f32 + 12.;
            widgets.panel(
                world,
                camera,
                "instruction",
                rect(x, y, 340., 28.),
                theme.panel,
                40,
            );
            widgets.text(
                world,
                camera,
                "instruction-text",
                rect(x + 10., y + 4., 320., 20.),
                crate::native_viewport::localization::translate(world, "sketch.pickPlanePrompt"),
                12.,
                41,
            );
        }
        if let Some(face) = picker.face {
            let size = crate::native_viewport::interface_shell::window_ui_size(world)
                .unwrap_or(Vec2::new(1440., 860.));
            let (width, height) = (size.x, size.y);
            let w = 360_f32.min(width - 24.);
            let x = (width - w) / 2.;
            let y = ((height - 308.) / 2.).max(0.);
            widgets.panel(
                world,
                camera,
                "shade",
                rect(0., 0., width, height),
                Color::BLACK.with_alpha(0.35),
                80,
            );
            let mut bounds = rect(x, y, w, 308.);
            bounds.border = UiRect::all(px(1.));
            bounds.border_radius = BorderRadius::all(px(6.));
            widgets.panel(
                world,
                camera,
                "panel",
                bounds,
                theme.panel.with_alpha(1.),
                81,
            );
            widgets.panel(
                world,
                camera,
                "header-rule",
                rect(x + 1., y + 40., w - 2., 1.),
                theme.edge,
                82,
            );
            widgets.panel(
                world,
                camera,
                "footer-rule",
                rect(x + 1., y + 260., w - 2., 1.),
                theme.edge,
                82,
            );
            if let Some(e) = widgets.entity("panel") {
                world.entity_mut(e).insert(BorderColor::all(theme.accent));
            }
            widgets.glyph(
                (world, camera),
                "icon",
                rect(x + 12., y + 12., 15., 15.),
                Icon::Crosshair,
                theme.accent,
                82,
            );
            widgets.text(
                world,
                camera,
                "title",
                rect(x + 36., y + 10., w - 66., 20.),
                crate::native_viewport::localization::translate(world, "sketchOrigin.title"),
                12.,
                82,
            );
            widgets.text(
                world,
                camera,
                "description",
                rect(x + 16., y + 50., w - 32., 36.),
                &crate::native_viewport::localization::translate(world, "sketchOrigin.description")
                    .replace("{face}", &face.0.to_string()),
                12.,
                82,
            );
            let mut button = |world: &mut World,
                              key: &str,
                              label: &str,
                              caption: Option<&str>,
                              command: EditorCommand,
                              node: Node,
                              selected: Option<bool>| {
                let mut c = InterfaceControl::button("sketch/draw", label);
                c.modal_scope = Some("sketch-origin".into());
                c.selected = selected;
                if selected.is_some() {
                    c.role = "radio".into();
                }
                widgets.button(
                    world,
                    camera,
                    key,
                    c,
                    caption,
                    NativeCommand::Sketch(command),
                    node,
                    (key == "close").then_some(Icon::Cancel),
                    83,
                )
            };
            let mut cancel_bounds = rect(x + w - 208., y + 269., 70., 28.);
            cancel_bounds.border = UiRect::all(px(1.));
            button(
                world,
                "close",
                crate::native_viewport::localization::translate(world, "sketchOrigin.close"),
                Some(""),
                EditorCommand::Cancel,
                rect(x + w - 30., y + 8., 24., 24.),
                None,
            )?;
            for (i, value, label) in [
                (
                    0,
                    FaceSketchOrigin::FaceCenter,
                    crate::native_viewport::localization::translate(
                        world,
                        "sketchOrigin.faceCenter",
                    ),
                ),
                (
                    1,
                    FaceSketchOrigin::GlobalOriginProjection,
                    crate::native_viewport::localization::translate(
                        world,
                        "sketchOrigin.globalProjection",
                    ),
                ),
            ] {
                let mut node = rect(x + 16., y + 98. + i as f32 * 66., w - 32., 56.);
                node.border = UiRect::all(px(1.));
                let e = button(
                    world,
                    &format!("origin-{i}"),
                    label,
                    None,
                    EditorCommand::Support(Command::Origin(value)),
                    node,
                    Some(picker.origin == value),
                )?;
                interface_shell::radio_card(world, e, camera, picker.origin == value);
            }
            button(
                world,
                "cancel",
                crate::native_viewport::localization::translate(
                    world,
                    "sketchOrigin.cancelPlacement",
                ),
                Some(crate::native_viewport::localization::translate(
                    world,
                    "sketchOrigin.cancel",
                )),
                EditorCommand::Cancel,
                cancel_bounds,
                None,
            )?;
            let e = button(
                world,
                "confirm",
                crate::native_viewport::localization::translate(world, "sketchOrigin.confirm"),
                Some(crate::native_viewport::localization::translate(
                    world,
                    "sketchOrigin.ok",
                )),
                EditorCommand::Support(Command::Confirm),
                rect(x + w - 130., y + 269., 118., 28.),
                None,
            )?;
            interface_shell::primary_button(world, e);
            for (i, hint) in [
                (
                    0,
                    crate::native_viewport::localization::translate(
                        world,
                        "sketchOrigin.faceCenterHint",
                    ),
                ),
                (
                    1,
                    crate::native_viewport::localization::translate(
                        world,
                        "sketchOrigin.globalProjectionHint",
                    ),
                ),
            ] {
                widgets.text(
                    world,
                    camera,
                    &format!("origin-hint-{i}"),
                    rect(x + 46., y + 126. + i as f32 * 66., w - 66., 25.),
                    hint,
                    10.,
                    84,
                );
            }
        }
        widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(panel);
    result
}

#[cfg(test)]
mod scaled_tests {
    use super::*;

    #[test]
    fn sketch_origin_controls_remain_visible_at_largest_interface_size_and_high_dpi() {
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
        let editor = Editor {
            support: Picker {
                face: Some(FaceId(7)),
                ..default()
            },
            ..default()
        };
        synchronize(&mut world, camera, &editor, InterfaceRect::default()).unwrap();
        let pixels = |value| match value {
            Val::Px(value) => value,
            _ => panic!("Expected UI pixels"),
        };
        let mut count = 0;
        for (control, node) in world.query::<(&InterfaceControl, &Node)>().iter(&world) {
            if control.modal_scope.as_deref() != Some("sketch-origin") {
                continue;
            }
            count += 1;
            assert!(
                pixels(node.left) >= 0. && pixels(node.top) >= 0.,
                "{}",
                control.label
            );
            assert!(
                pixels(node.left) + pixels(node.width) <= 1200. / 1.75 + 0.01,
                "{}",
                control.label
            );
            assert!(
                pixels(node.top) + pixels(node.height) <= 760. / 1.75 + 0.01,
                "{}",
                control.label
            );
        }
        assert_eq!(
            count, 5,
            "Both origin choices, Close, Cancel, and Create Sketch must remain reachable"
        );
    }
}
