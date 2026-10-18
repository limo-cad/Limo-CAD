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
    let choice_height = if choices { 36. } else { 0. };
    let trail_height = if demo.is_some() { 32. } else { 0. };
    let image_height = ((image_width * 0.72).min((height - y - 200. - trail_height).max(64.))
        - choice_height)
        .max(64.);
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
    let mut palette = crate::native_viewport::ui::palette(world);
    // Quiet only the teaching stage; the document viewport keeps its grid.
    if demo.is_some() {
        palette.grid_fine = palette.background;
        palette.grid_major = palette.background;
    }
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
    card.paint(
        world,
        camera,
        widgets,
        "demo",
        image_height + choice_height + trail_height + 192.,
        demo.map_or(t("ui.showMe"), |d| d.title(language)),
        theme,
    )?;
    let expanded = world.resource::<Files>().script.preview.steps_open;
    if expanded {
        let label = format!("‹  {}", t("ui.hideSteps"));
        let mut control = InterfaceControl::button("document/scripts", &label);
        control.expanded = Some(true);
        widgets.button(
            world,
            camera,
            "lesson-all-steps",
            control,
            Some(&label),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Steps)),
            Node {
                border: UiRect::all(px(1.)),
                ..rect(x + 12., y + 44., w - 104., 32.)
            },
            None,
            61,
        )?;
        widgets.button(
            world,
            camera,
            "extrude-open-help",
            InterfaceControl::button("document/scripts", t("ui.help")),
            Some(t("ui.help")),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Help)),
            rect(x + w - 84., y + 44., 68., 32.),
            None,
            61,
        )?;
        widgets.text(
            world,
            camera,
            "lesson-steps-heading",
            rect(x + 16., y + 84., w - 32., 24.),
            t("ui.chooseStep"),
            14.,
            61,
        );
        steps::paint(
            world,
            camera,
            widgets,
            x + 12.,
            y + 112.,
            image_width,
            image_height + choice_height + trail_height + 64.,
            true,
        )?;
        return Ok(());
    } else {
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
                border: UiRect::ZERO,
                border_radius: BorderRadius::all(px(8.)),
                overflow: Overflow::clip(),
                ..rect(x + 12., y + 44., image_width, image_height)
            },
            None,
            61,
        )?;
        if !world.resource::<Files>().script.preview.steps_return_focus {
            feature::focus_control(world, entity);
        }
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
        annotations::paint(
            world,
            camera,
            widgets,
            entity,
            [image_width, image_height],
            current,
        )?;
    }
    let trail_top = y + 50. + image_height;
    if demo.is_some() && !expanded {
        steps::paint(
            world,
            camera,
            widgets,
            x + 12.,
            trail_top,
            image_width,
            28.,
            false,
        )?;
    }
    let top = y + 54. + image_height + trail_height;
    // Keep the full sequence one action away from the local step trail.
    if demo.is_some() {
        widgets.text(
            world,
            camera,
            "lesson-step-count",
            rect(x + 16., top, 44., 24.),
            &format!("{} / {}", index + 1, count.max(1)),
            12.,
            61,
        );
        let label = format!("{}  ▾", t("ui.allSteps"));
        let mut control = InterfaceControl::button("document/scripts", t("ui.allSteps"));
        control.disabled = !ready;
        control.expanded = Some(false);
        let entity = widgets.button(
            world,
            camera,
            "lesson-all-steps",
            control,
            Some(&label),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Steps)),
            Node {
                border: UiRect::all(px(1.)),
                ..rect(x + 64., top - 4., w - 156., 32.)
            },
            None,
            61,
        )?;
        if world.resource::<Files>().script.preview.steps_return_focus {
            feature::focus_control(world, entity);
            if world
                .resource::<Files>()
                .script
                .preview
                .focus_owner
                .is_none()
            {
                world
                    .resource_mut::<Files>()
                    .script
                    .preview
                    .steps_return_focus = false;
            }
        }
    }
    if demo.is_some() {
        widgets.button(
            world,
            camera,
            "extrude-open-help",
            InterfaceControl::button("document/scripts", t("ui.help")),
            Some(t("ui.help")),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Help)),
            rect(x + w - 84., top - 4., 68., 24.),
            None,
            61,
        )?;
    }
    widgets.text(
        world,
        camera,
        "scripts-preview-caption",
        rect(x + 16., top + 36., w - 32., 52.),
        &caption,
        14.,
        61,
    );
    let buttons = top + 92.;
    let mut previous = InterfaceControl::button("document/scripts", t("ui.previous"));
    previous.disabled = !ready || index == 0;
    widgets.button(
        world,
        camera,
        "scripts-preview-button-0",
        previous,
        Some("‹"),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Previous)),
        rect(x + 12., buttons, 36., 32.),
        None,
        61,
    )?;
    let (label, action) = if playing {
        (t("ui.pause"), Action::Stop)
    } else if index + 1 < count {
        (t("ui.next"), Action::Next)
    } else {
        (t("ui.replay"), Action::Replay)
    };
    let mut primary = InterfaceControl::button("document/scripts", label);
    primary.disabled = !ready;
    primary.selected = Some(true);
    widgets.button(
        world,
        camera,
        "scripts-preview-primary",
        primary,
        Some(label),
        NativeCommand::File(FileCommand::ScriptPreview(action)),
        Node {
            justify_content: JustifyContent::Center,
            ..rect(x + 56., buttons, w - 68., 32.)
        },
        None,
        61,
    )?;
    // Offer the second outcome only after the first has been understood.
    if choices && ready && index + 1 == count {
        let scenario = world.resource::<Files>().script.preview.scenario;
        let other = lesson::CutScenario::ALL
            .into_iter()
            .find(|s| *s != scenario)
            .unwrap();
        let label = format!("{} {}", t("ui.try"), other.label(language));
        widgets.button(
            world,
            camera,
            "cut-scenario-alternative",
            InterfaceControl::button("document/scripts", &label),
            Some(&label),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Scenario(other))),
            rect(x + 12., buttons + 38., w - 24., 28.),
            None,
            61,
        )?;
    }
    Ok(())
}
