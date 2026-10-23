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
    let demo = lesson::current(world);
    let language = lesson::language(world);
    let t = |key: &str| limo_cad_mcp::lessons::text(language, key);
    let card = card::Card::new(width);
    let (x, y, w) = (card.x, card.y, card.width);
    let image_width = w - 24.;
    let choices = world.resource::<Files>().script.preview.lesson == Some(lesson::Id::Cut);
    let instructions = world.resource::<Files>().script.preview.help;
    let side_instructions = instructions && width >= 680.;
    let choice_height = if choices { 36. } else { 0. };
    let trail_height = if demo.is_some() { 32. } else { 0. };
    let instructions_height = if instructions && !side_instructions {
        (height - y - 16. - (64. + choice_height + trail_height + 192.)).clamp(0., 148.)
    } else {
        0.
    };
    let image_height = ((image_width * 0.72)
        .min((height - y - 200. - trail_height - instructions_height).max(64.))
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
    let index = preview.display_index();
    let (count, playing) = (preview.count(), preview.playing);
    let terminal = index + 1 == count && preview.animation_complete();
    let image = preview.image.clone();
    let current = preview.image_ready();
    let blend = preview
        .blend
        .as_ref()
        .filter(|blend| current && blend.target == preview.frame_index())
        .map(|blend| (blend.from.clone(), blend.alpha()));
    let ready = preview.retained.is_some();
    let caption = preview.error.clone().unwrap_or_else(|| {
        preview
            .retained
            .as_ref()
            .map(|r| {
                limo_cad_mcp::lessons::caption(
                    language,
                    &r.descriptor.captions[preview.display_frame_index()],
                )
            })
            .unwrap_or_else(|| t("ui.preparing").into())
    });
    let card_height = image_height + choice_height + trail_height + 192. + instructions_height;
    let shell = if side_instructions {
        card::Card {
            x: x - 288.,
            y,
            width: w + 288.,
        }
    } else {
        card
    };
    shell.paint(
        world,
        camera,
        widgets,
        "demo",
        card_height,
        demo.map_or(t("ui.guidedExamples"), |d| {
            t(&format!("{}.example", d.key()))
        }),
        theme,
    )?;
    if instructions {
        let bounds = if side_instructions {
            widgets.panel(
                world,
                camera,
                "guide-instructions-divider",
                rect(x - 8., y + 44., 1., card_height - 56.),
                theme.edge,
                61,
            );
            [shell.x + 12., y + 44., 264., card_height - 56.]
        } else {
            let top = y + card_height - instructions_height;
            widgets.panel(
                world,
                camera,
                "guide-instructions-divider",
                rect(x + 12., top, w - 24., 1.),
                theme.edge,
                61,
            );
            [
                x + 12.,
                top + 8.,
                w - 24.,
                (instructions_height - 16.).max(32.),
            ]
        };
        feature::paint_instructions(world, camera, widgets, bounds, theme)?;
    }
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
                ..rect(x + 12., y + 44., w - 164., 32.)
            },
            None,
            61,
        )?;
        let mut instructions_control = InterfaceControl::button(
            "document/scripts",
            t(if instructions {
                "ui.hideInstructions"
            } else {
                "ui.instructions"
            }),
        );
        instructions_control.expanded = Some(instructions);
        widgets.button(
            world,
            camera,
            "extrude-open-help",
            instructions_control,
            Some(t("ui.instructions")),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Help)),
            rect(x + w - 140., y + 44., 128., 32.),
            Some(interface_shell::ribbon::Icon::Book),
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
        if !world.resource::<Files>().script.preview.steps_return_focus
            && !world
                .resource::<Files>()
                .script
                .preview
                .help_view
                .return_focus
        {
            feature::focus_control(world, entity);
        }
        let base_image = blend
            .as_ref()
            .map(|(from, _)| from.clone())
            .or_else(|| image.clone());
        if let Some(image) = base_image {
            if world
                .get::<ImageNode>(entity)
                .is_none_or(|node| node.image != image)
            {
                world.entity_mut(entity).insert(ImageNode::new(image));
            }
        } else if world.get::<ImageNode>(entity).is_some() {
            world.entity_mut(entity).remove::<ImageNode>();
        }
        if let (Some((_, alpha)), Some(image)) = (blend, image) {
            // Decorative child: the model remains the sole pointer/key/focus
            // target. The transparent background must not cover its old image.
            // Model and blend use 61; dimension leaders/labels stay above at 62/63.
            widgets.panel(
                world,
                camera,
                "scripts-preview-blend",
                Node {
                    border_radius: BorderRadius::all(px(8.)),
                    ..rect(0., 0., image_width, image_height)
                },
                Color::NONE,
                61,
            );
            let overlay = widgets.entity("scripts-preview-blend").unwrap();
            world
                .entity_mut(overlay)
                .remove::<interface_shell::InterfaceOccluder>()
                .insert((
                    ChildOf(entity),
                    interface_shell::InterfacePointerPassthrough,
                    ImageNode {
                        color: Color::srgba(1., 1., 1., alpha),
                        ..ImageNode::new(image)
                    },
                ));
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
                ..rect(x + 60., top - 4., w - 208., 32.)
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
        let mut control = InterfaceControl::button(
            "document/scripts",
            t(if instructions {
                "ui.hideInstructions"
            } else {
                "ui.instructions"
            }),
        );
        control.expanded = Some(instructions);
        let instructions_label = format!(
            "{} {}",
            t("ui.instructions"),
            if instructions { "▴" } else { "▾" }
        );
        let instructions_button = widgets.button(
            world,
            camera,
            "extrude-open-help",
            control,
            Some(&instructions_label),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Help)),
            rect(x + w - 140., top - 4., 128., 32.),
            Some(interface_shell::ribbon::Icon::Book),
            61,
        )?;
        if world
            .resource::<Files>()
            .script
            .preview
            .help_view
            .return_focus
        {
            feature::focus_control(world, instructions_button);
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
                    .help_view
                    .return_focus = false;
            }
        }
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
    } else if terminal {
        (t("ui.replay"), Action::Replay)
    } else {
        (t("ui.resume"), Action::Resume)
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
            ..rect(x + 56., buttons, w - 156., 32.)
        },
        None,
        61,
    )?;
    let mut next = InterfaceControl::button("document/scripts", t("ui.next"));
    next.disabled = !ready || index + 1 >= count;
    widgets.button(
        world,
        camera,
        "scripts-preview-next",
        next,
        Some(t("ui.next")),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Next)),
        rect(x + w - 92., buttons, 80., 32.),
        None,
        61,
    )?;
    // Offer the second outcome only after the first has been understood.
    if choices && ready && terminal {
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
