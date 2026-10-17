use super::super::super::super::chrome::{self, rect};
use super::*;

pub(crate) fn paint(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    height: f32,
    theme: ViewportUiTheme,
) -> Result<(), String> {
    if world.resource::<Files>().script.preview.index_open {
        return feature::paint_index(world, camera, widgets, width, height, theme);
    }
    if world.resource::<Files>().script.preview.help {
        return feature::paint_help(world, camera, widgets, width, height, theme);
    }
    let demo = lesson::current(world);
    let language = lesson::language(world);
    let t = |key: &str| limo_cad_mcp::lessons::text(language, key);
    let card = card::Card::new(width);
    let (x, y, w) = (card.x, card.y, card.width);
    let image_width = w - 24.;
    let choices = world.resource::<Files>().script.preview.lesson == Some(lesson::Id::Cut);
    let choice_height = if choices { 28. } else { 0. };
    let image_height =
        ((image_width * 176. / 300.).min((height - y - 204.).max(64.)) - choice_height).max(64.);
    let scale = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map_or(1., Window::scale_factor)
        * world
            .get_resource::<bevy::ui::UiScale>()
            .map_or(1., |s| s.0);
    let scale = scale
        .clamp(1., 2.)
        .min(MAX_RENDER_SIZE[0] as f32 / image_width)
        .min(MAX_RENDER_SIZE[1] as f32 / image_height);
    let size = [
        ((image_width * scale).round() as u32).max(64),
        ((image_height * scale).round() as u32).max(64),
    ];
    let palette = crate::native_viewport::ui::palette(world);
    let preview = &mut world.resource_mut::<Files>().script.preview;
    if preview.palette != palette {
        preview.palette = palette;
        preview.changed()?;
    }
    if preview.size != size {
        preview.size = size;
        preview.changed()?;
    }
    let (index, count, playing) = (preview.index, preview.count(), preview.playing);
    let image = (preview.image_index == Some(index))
        .then(|| preview.image.clone())
        .flatten();
    let current = image.is_some() && preview.rendered == preview.revision;
    let ready = preview.retained.is_some();
    let caption = preview.error.clone().unwrap_or_else(|| {
        preview
            .retained
            .as_ref()
            .map(|r| {
                limo_cad_mcp::lessons::caption(
                    language,
                    &r.descriptor.captions[preview.frame_index()],
                )
            })
            .unwrap_or_else(|| t("ui.preparing").into())
    });
    let cue = preview
        .retained
        .as_ref()
        .and_then(|r| r.descriptor.cues.get(preview.frame_index()))
        .copied()
        .flatten();
    card.paint(
        world,
        camera,
        widgets,
        "demo",
        image_height + choice_height + if demo.is_some() { 188. } else { 162. },
        demo.map_or(t("ui.showMe"), |d| d.title(language)),
        theme,
    )?;
    if choices {
        let scenario = world.resource::<Files>().script.preview.scenario;
        for (i, option) in lesson::CutScenario::ALL.into_iter().enumerate() {
            let mut control = InterfaceControl::button("document/scripts", option.label(language));
            control.selected = Some(option == scenario);
            control.disabled = !ready;
            widgets.button(
                world,
                camera,
                &format!("cut-scenario-{}", option.key()),
                control,
                Some(option.label(language)),
                NativeCommand::File(FileCommand::ScriptPreview(Action::Scenario(option))),
                rect(
                    x + 12. + i as f32 * (image_width + 4.) / 2.,
                    y + 40.,
                    (image_width - 4.) / 2.,
                    24.,
                ),
                None,
                62,
            )?;
        }
    }
    let mut model = InterfaceControl::button(
        "document/scripts",
        format!(
            "{}: {}{caption}",
            t("ui.modelHint"),
            if current { "" } else { t("ui.preparing") }
        ),
    );
    model.owned_keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home"]
        .map(limo_cad_interface::KeyChord::plain)
        .into();
    let entity = widgets.button(
        world,
        camera,
        "scripts-preview-model",
        model,
        Some(if image.is_some() { "" } else { t("ui.loading") }),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Model)),
        Node {
            border: UiRect::all(px(1.)),
            border_radius: BorderRadius::all(px(4.)),
            ..rect(x + 12., y + 44. + choice_height, image_width, image_height)
        },
        None,
        61,
    )?;
    feature::focus_control(world, entity);
    if let Some(image) = image {
        if world
            .get::<ImageNode>(entity)
            .is_none_or(|node| node.image != image)
        {
            world.entity_mut(entity).insert(ImageNode::new(image));
        }
    } else if world.get::<ImageNode>(entity).is_some() {
        world.entity_mut(entity).remove::<ImageNode>();
    }
    let top = y + 54. + choice_height + image_height;
    let extra = if let Some(demo) = demo {
        feature::guide(
            world,
            camera,
            widgets,
            (x + 12., top, w - 24.),
            demo,
            cue,
            theme,
        );
        26.
    } else {
        0.
    };
    widgets.text(
        world,
        camera,
        "scripts-preview-caption",
        rect(x + 12., top + extra, w - 24., 38.),
        &caption,
        12.,
        61,
    );
    let buttons = top + extra + 44.;
    for (i, (label, caption, action, disabled, left, width)) in [
        (
            t("ui.previous"),
            "‹",
            Action::Previous,
            !ready || index == 0,
            x + 8.,
            28.,
        ),
        (
            if playing {
                t("ui.pause")
            } else {
                t("ui.replay")
            },
            if playing {
                t("ui.pause")
            } else {
                t("ui.replay")
            },
            if playing {
                Action::Stop
            } else {
                Action::Replay
            },
            count < 2,
            x + 36.,
            64.,
        ),
        (
            t("ui.next"),
            "›",
            Action::Next,
            !ready || index + 1 >= count,
            x + 100.,
            28.,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut control = InterfaceControl::button("document/scripts", label);
        control.disabled = disabled;
        widgets.button(
            world,
            camera,
            &format!("scripts-preview-button-{i}"),
            control,
            Some(caption),
            NativeCommand::File(FileCommand::ScriptPreview(action)),
            Node {
                justify_content: JustifyContent::Center,
                ..rect(left, buttons, width, 28.)
            },
            None,
            61,
        )?;
    }
    widgets.button(
        world,
        camera,
        "scripts-preview-fit",
        InterfaceControl::button("document/scripts", t("ui.fit")),
        Some(""),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Fit)),
        rect(x + w - 116., buttons, 28., 28.),
        Some(interface_shell::ribbon::Icon::Fit),
        61,
    )?;
    if demo.is_some() {
        widgets.button(
            world,
            camera,
            "extrude-open-help",
            InterfaceControl::button("document/scripts", t("ui.help")),
            Some(t("ui.help")),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Help)),
            rect(x + w - 84., buttons, 72., 28.),
            Some(interface_shell::ribbon::Icon::Book),
            61,
        )?;
    }
    widgets.colored_text(
        world,
        camera,
        "scripts-preview-safety",
        rect(x + 12., buttons + 36., w - 24., 16.),
        t("ui.footer"),
        10.,
        61,
        theme.mute,
    );
    Ok(())
}
