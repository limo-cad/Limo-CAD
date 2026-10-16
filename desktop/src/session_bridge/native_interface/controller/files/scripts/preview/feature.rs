//! Contextual entry and guidance for one real, isolated Extrude example.
use super::super::super::super::chrome::{self, rect};
use super::*;
use crate::native_viewport::script_preview::PreviewCue;
use std::sync::OnceLock;

#[derive(Resource, Default)]
struct Hint(bool);

fn contains(point: Vec2, bounds: [f32; 4]) -> bool {
    point.x >= bounds[0]
        && point.y >= bounds[1]
        && point.x <= bounds[0] + bounds[2]
        && point.y <= bounds[1] + bounds[3]
}

fn keep_hint(point: Vec2, anchor: [f32; 4], card: [f32; 4], open: bool) -> bool {
    contains(point, anchor)
        || (open
            && (contains(point, card)
                || contains(
                    point,
                    [
                        anchor[0],
                        anchor[1] + anchor[3],
                        anchor[2],
                        (card[1] - anchor[1] - anchor[3]).max(0.),
                    ],
                )))
}

pub(crate) fn paint_hint(
    world: &mut World,
    camera: Entity,
    extrude: Option<Entity>,
    widgets: &mut chrome::Widgets,
    width: f32,
) -> Result<(), String> {
    world.init_resource::<Hint>();
    let allowed = !world.resource::<Files>().script.preview.open
        && super::super::super::modal(world).is_none()
        && workbench::modal(world).is_none()
        && world
            .get_resource::<NativeInterfaceHandle>()
            .and_then(NativeInterfaceHandle::presented_pointer_context)
            .is_none_or(|(_, modal_empty, _)| modal_empty)
        && crate::session_bridge::native_interface::feature::panel(world).is_none();
    let anchor = extrude
        .filter(|entity| {
            allowed
                && world
                    .get::<InterfaceControl>(*entity)
                    .is_some_and(|c| c.visible)
        })
        .and_then(|entity| {
            let node = world.get::<Node>(entity)?;
            match (node.left, node.top, node.width, node.height) {
                (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h)) => Some((entity, [x, y, w, h])),
                _ => None,
            }
        });
    let Some((entity, anchor)) = anchor else {
        world.resource_mut::<Hint>().0 = false;
        return Ok(());
    };
    let w = (width - 24.).clamp(260., 280.);
    let x = anchor[0].min((width - w - 8.).max(4.));
    let y = anchor[1] + anchor[3] + 6.;
    let scale = world
        .get_resource::<bevy::ui::UiScale>()
        .map_or(1., |s| s.0);
    let point = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .find(|window| window.focused)
        .and_then(Window::cursor_position)
        .map(|p| p / scale);
    let focused = world
        .get_resource::<NativeInterfaceHandle>()
        .and_then(NativeInterfaceHandle::focused_key)
        .is_some_and(|key| {
            key.0 == entity.to_bits()
                || widgets
                    .entity("extrude-show-me")
                    .is_some_and(|button| key.0 == button.to_bits())
        });
    let open = focused
        || point.is_some_and(|p| keep_hint(p, anchor, [x, y, w, 100.], world.resource::<Hint>().0));
    world.resource_mut::<Hint>().0 = open;
    if !open {
        return Ok(());
    }
    let theme = crate::native_viewport::ui::theme(world);
    widgets.panel(
        world,
        camera,
        "extrude-help-card",
        Node {
            border: UiRect::all(px(1.)),
            border_radius: BorderRadius::all(px(6.)),
            ..rect(x, y, w, 100.)
        },
        theme.panel.with_alpha(1.),
        65,
    );
    widgets.text(
        world,
        camera,
        "extrude-help-title",
        rect(x + 12., y + 8., w - 24., 20.),
        "Extrude",
        13.,
        66,
    );
    widgets.text(
        world,
        camera,
        "extrude-help-description",
        rect(x + 12., y + 30., w - 24., 32.),
        "Give a closed sketch profile depth to make a solid.",
        12.,
        66,
    );
    let mut control = InterfaceControl::button("solid/build", "Show me how to Extrude");
    control.disabled = available(world).is_err();
    let entry = widgets.button(
        world,
        camera,
        "extrude-show-me",
        control,
        Some("Show me"),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Extrude)),
        rect(x + 8., y + 66., 100., 26.),
        Some(interface_shell::ribbon::Icon::Book),
        66,
    )?;
    if let Some(handle) = world.get_resource::<NativeInterfaceHandle>() {
        handle.place_in_tab_order_after(
            limo_cad_interface::ControlKey(entry.to_bits()),
            limo_cad_interface::ControlKey(entity.to_bits()),
        )?;
    }
    Ok(())
}

pub(super) fn guide(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    (x, y, w): (f32, f32, f32),
    demo: &lesson::Demo,
    cue: Option<PreviewCue>,
    theme: ViewportUiTheme,
) {
    let width = (w - 12.) / 4.;
    for (index, (step, label)) in demo.steps.iter().enumerate() {
        let left = x + index as f32 * (width + 4.);
        let selected = cue == Some(*step);
        widgets.colored_text(
            world,
            camera,
            &format!("extrude-cue-label-{index}"),
            rect(left, y, width, 20.),
            label,
            10.,
            63,
            if selected { theme.ink } else { theme.mute },
        );
        widgets.panel(
            world,
            camera,
            &format!("extrude-cue-{index}"),
            rect(left, y + 19., width, 2.),
            if selected { theme.accent } else { theme.edge },
            62,
        );
    }
}
pub(super) fn help() -> &'static Result<Vec<String>, String> {
    static PAGE: OnceLock<Result<Vec<String>, String>> = OnceLock::new();
    PAGE.get_or_init(|| {
        let page = limo_cad_mcp::help_page(lesson::EXTRUDE.help)?;
        let body = page["body"].as_str().ok_or("Extrude help has no content")?;
        Ok(body
            .split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect())
    })
}

/// Resolve only after layout publishes this control, against the original owner.
pub(super) fn focus_control(world: &mut World, entity: Entity) {
    let Some(owner) = world.resource::<Files>().script.preview.focus_owner.clone() else {
        return;
    };
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() else {
        return;
    };
    if !handle.presented_focus_matches(&owner, None) {
        world.resource_mut::<Files>().script.preview.focus_owner = None;
        return;
    }
    if let Ok(action) = handle.resolve_retained(limo_cad_interface::ControlKey(entity.to_bits())) {
        if action.context == owner && handle.prepare_activation(&action).is_ok() {
            world.resource_mut::<Files>().script.preview.focus_owner = None;
            return;
        }
    }
    handle.request_redraw();
}

#[derive(Default)]
pub(super) struct HelpView {
    pub offset: f32,
    max: f32,
    bounds: [f32; 4],
}

pub(super) fn scroll_help_key(
    world: &mut World,
    key: &limo_cad_interface::KeyChord,
) -> Result<(), String> {
    if key.ctrl || key.alt || key.meta || key.shift {
        return Err("Use unmodified navigation keys to scroll help".into());
    }
    let preview = &mut world.resource_mut::<Files>().script.preview;
    if !preview.open || !preview.help {
        return Err("Open help before scrolling".into());
    }
    let view = &mut preview.help_view;
    let page = view.bounds[3] * 0.8;
    view.offset = match key.key.as_str() {
        "ArrowUp" => view.offset - 28.,
        "ArrowDown" => view.offset + 28.,
        "PageUp" => view.offset - page,
        "PageDown" => view.offset + page,
        "Home" => 0.,
        "End" => view.max,
        _ => return Err("Use arrows, Page Up/Down, Home or End to scroll help".into()),
    }
    .clamp(0., view.max);
    Ok(())
}

pub(super) fn help_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &NativeHostInput,
) -> Result<bool, String> {
    if !world.resource::<Files>().script.preview.help
        || !handle
            .presented_pointer_context()
            .is_some_and(|(owner, empty, _)| empty && event.context.as_ref() == Some(&owner))
    {
        return Ok(false);
    }
    let WindowEvent::MouseWheel(wheel) = &event.event else {
        return Ok(false);
    };
    let bounds = world.resource::<Files>().script.preview.help_view.bounds;
    if !event.cursor.is_some_and(|point| contains(point, bounds)) {
        return Ok(false);
    }
    let scale = world
        .get::<Window>(wheel.window)
        .map_or(1., |w| w.resolution.scale_factor());
    let amount = match wheel.unit {
        bevy::input::mouse::MouseScrollUnit::Line => wheel.y * 28.,
        bevy::input::mouse::MouseScrollUnit::Pixel => {
            wheel.y / (scale * handle.presented_ui_scale())
        }
    };
    if amount.is_finite() {
        let view = &mut world.resource_mut::<Files>().script.preview.help_view;
        view.offset = (view.offset - amount).clamp(0., view.max);
        handle.request_redraw();
    }
    Ok(true)
}

pub(super) fn paint_help(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    height: f32,
    theme: ViewportUiTheme,
) -> Result<(), String> {
    let card = card::Card::new(width);
    let (x, y, w) = (card.x, card.y, card.width);
    let h = 364_f32.min((height - y - 16.).max(180.));
    card.paint(
        world,
        camera,
        widgets,
        "help",
        h,
        lesson::EXTRUDE.title,
        theme,
    )?;
    let body = match help() {
        Ok(paragraphs) => paragraphs.join("\n\n"),
        Err(error) => error.clone(),
    };
    let bounds = [x + 12., y + 44., w - 28., h - 96.];
    let content_height = widgets
        .entity("extrude-help-text")
        .and_then(|entity| world.get::<ComputedNode>(entity))
        .map_or(0., |node| node.size.y * node.inverse_scale_factor);
    let view = &mut world.resource_mut::<Files>().script.preview.help_view;
    view.bounds = bounds;
    view.max = (content_height - bounds[3]).max(0.);
    view.offset = view.offset.clamp(0., view.max);
    let (offset, max) = (view.offset, view.max);
    let mut control = InterfaceControl::button(
        "document/scripts",
        "Extrude help; scroll with arrows or Page Up and Page Down",
    );
    control.role = "region".into();
    control.owned_keys = ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"]
        .map(limo_cad_interface::KeyChord::plain)
        .into();
    let clip = widgets.button(
        world,
        camera,
        "extrude-help-body",
        control,
        Some(""),
        NativeCommand::File(FileCommand::ScriptPreview(Action::HelpBody)),
        rect(bounds[0], bounds[1], bounds[2], bounds[3]),
        None,
        61,
    )?;
    widgets.text(
        world,
        camera,
        "extrude-help-text",
        Node {
            height: Val::Auto,
            align_items: AlignItems::Start,
            ..rect(0., -offset, bounds[2] - 4., 0.)
        },
        &body,
        12.,
        1,
    );
    widgets.parent(world, "extrude-help-text", clip);
    if max > 0. {
        let thumb_height = (bounds[3] * bounds[3] / content_height).max(20.);
        widgets.panel(
            world,
            camera,
            "extrude-help-scrollbar",
            rect(
                x + w - 10.,
                bounds[1] + (bounds[3] - thumb_height) * offset / max,
                2.,
                thumb_height,
            ),
            theme.mute.with_alpha(0.6),
            62,
        );
    }
    widgets.button(
        world,
        camera,
        "extrude-help-back",
        InterfaceControl::button("document/scripts", "Back to demonstration"),
        Some("Back to demonstration"),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Demo)),
        rect(x + 8., y + h - 40., w - 16., 28.),
        None,
        61,
    )?;
    focus_control(world, clip);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_script_preview_help_scroll_is_bounded_and_preserves_navigation() {
        let mut world = World::new();
        initialize(
            &mut world,
            Arc::new(Mutex::new(DocumentWorkspace::default())),
        );
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.open = true;
        preview.help = true;
        preview.help_view.bounds = [0., 0., 280., 120.];
        preview.help_view.max = 200.;
        let key = limo_cad_interface::KeyChord::plain;
        scroll_help_key(&mut world, &key("PageDown")).unwrap();
        assert_eq!(
            world.resource::<Files>().script.preview.help_view.offset,
            96.
        );
        scroll_help_key(&mut world, &key("End")).unwrap();
        scroll_help_key(&mut world, &key("ArrowDown")).unwrap();
        assert_eq!(
            world.resource::<Files>().script.preview.help_view.offset,
            200.
        );
        scroll_help_key(&mut world, &key("Home")).unwrap();
        scroll_help_key(&mut world, &key("ArrowUp")).unwrap();
        assert_eq!(
            world.resource::<Files>().script.preview.help_view.offset,
            0.
        );
        assert!(scroll_help_key(&mut world, &key("Enter")).is_err());
        assert!(world.resource::<Files>().script.preview.help);
        world.resource_mut::<Files>().script.preview.open = false;
        assert!(scroll_help_key(&mut world, &key("PageDown")).is_err());
    }
    #[test]
    fn native_script_preview_keyboard_can_tab_into_show_me_without_a_pointer() {
        use bevy::ui::{ComputedStackIndex, UiGlobalTransform};
        let (mut app, handle, extrude, _) = interface_shell::tests::fixture();
        initialize(
            app.world_mut(),
            Arc::new(Mutex::new(DocumentWorkspace::default())),
        );
        app.world_mut().init_resource::<ViewportUiAssets>();
        app.world_mut()
            .entity_mut(extrude)
            .insert(rect(110., 34., 48., 54.));
        let camera = app.world_mut().spawn_empty().id();
        let mut widgets = chrome::Widgets::default();
        let action = handle
            .resolve_retained(limo_cad_interface::ControlKey(extrude.to_bits()))
            .unwrap();
        handle.prepare_activation(&action).unwrap();
        widgets.begin();
        paint_hint(app.world_mut(), camera, Some(extrude), &mut widgets, 1000.).unwrap();
        let entry = widgets.entity("extrude-show-me").unwrap();
        app.world_mut().entity_mut(entry).insert((
            ComputedNode {
                size: Vec2::new(288., 26.),
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(266., 181.)),
            ComputedStackIndex(10),
            InheritedVisibility::VISIBLE,
        ));
        app.update();
        assert!(handle.focus_next(false).unwrap());
        assert_eq!(
            handle.focused_key(),
            Some(limo_cad_interface::ControlKey(entry.to_bits()))
        );
        widgets.begin();
        paint_hint(app.world_mut(), camera, Some(extrude), &mut widgets, 1000.).unwrap();
        widgets.finish(app.world_mut());
        assert!(app.world().get::<InterfaceControl>(entry).is_some());
        assert!(handle
            .resolve_retained(limo_cad_interface::ControlKey(entry.to_bits()))
            .is_ok());
    }

    #[test]
    fn native_script_preview_hint_can_be_entered_without_running_or_latching_over_other_tools() {
        let anchor = [110., 34., 48., 54.];
        let card = [110., 94., 312., 112.];
        assert!(keep_hint(Vec2::new(125., 50.), anchor, card, false));
        assert!(keep_hint(Vec2::new(125., 91.), anchor, card, true));
        assert!(keep_hint(Vec2::new(250., 180.), anchor, card, true));
        assert!(!keep_hint(Vec2::new(250., 50.), anchor, card, true));
        assert!(!keep_hint(Vec2::new(600., 180.), anchor, card, true));
        assert!(!keep_hint(Vec2::new(250., 180.), anchor, card, false));
    }
}
