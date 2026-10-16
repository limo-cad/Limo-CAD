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
    let w = (width - 24.).clamp(260., 312.);
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
        || point.is_some_and(|p| keep_hint(p, anchor, [x, y, w, 136.], world.resource::<Hint>().0));
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
            border_radius: BorderRadius::all(px(8.)),
            ..rect(x, y, w, 136.)
        },
        theme.panel.with_alpha(1.),
        65,
    );
    widgets.text(
        world,
        camera,
        "extrude-help-title",
        rect(x + 16., y + 12., w - 32., 24.),
        "Extrude",
        16.,
        66,
    );
    widgets.text(
        world,
        camera,
        "extrude-help-description",
        rect(x + 16., y + 40., w - 32., 36.),
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
        rect(x + 16., y + 88., w - 32., 32.),
        None,
        66,
    )?;
    interface_shell::primary_button(world, entry);
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
    let width = (w - 18.) / 4.;
    for (index, (step, label)) in demo.steps.iter().enumerate() {
        let left = x + index as f32 * (width + 6.);
        let selected = cue == Some(*step);
        widgets.colored_text(
            world,
            camera,
            &format!("extrude-cue-label-{index}"),
            rect(left, y, width, 20.),
            &format!("{}  {label}", index + 1),
            11.,
            63,
            if selected { theme.ink } else { theme.mute },
        );
        widgets.panel(
            world,
            camera,
            &format!("extrude-cue-{index}"),
            rect(left, y + 24., width, 3.),
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
    let h = ((w - 32.) * 176. / 300.).min((height - y - 326.).max(24.)) + 310.;
    card.paint(
        world,
        camera,
        widgets,
        "help",
        h,
        lesson::EXTRUDE.title,
        theme,
    )?;
    widgets.text(
        world,
        camera,
        "extrude-help-page",
        rect(x + 16., y + 76., w - 32., 24.),
        "A closed profile becomes a solid",
        14.,
        61,
    );
    let body = match help() {
        Ok(paragraphs) => paragraphs.join("\n\n"),
        Err(error) => error.clone(),
    };
    widgets.text(
        world,
        camera,
        "extrude-help-body",
        rect(x + 16., y + 116., w - 32., h - 184.),
        &body,
        13.,
        61,
    );
    let entity = widgets.button(
        world,
        camera,
        "extrude-help-back",
        InterfaceControl::button("document/scripts", "Back to demonstration"),
        Some("Back to demonstration"),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Demo)),
        rect(x + 16., y + h - 48., w - 32., 32.),
        None,
        61,
    )?;
    interface_shell::primary_button(world, entity);
    focus_control(world, entity);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
