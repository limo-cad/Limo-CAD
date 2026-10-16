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
    if world.resource::<Files>().script.preview.help {
        return feature::paint_help(world, camera, widgets, width, height, theme);
    }
    let script = &world.resource::<Files>().script;
    let demo = (script.preview.return_to.is_some()
        || script
            .example
            .is_some_and(|e| e.id == lesson::EXTRUDE.recipe))
    .then_some(&lesson::EXTRUDE);
    let card = card::Card::new(width);
    let (x, y, w) = (card.x, card.y, card.width);
    let image_width = w - 32.;
    let image_height = (image_width * 176. / 300.).min((height - y - 326.).max(24.));
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
            .map(|r| r.descriptor.captions[index].clone())
            .unwrap_or_else(|| "Preparing your example…".into())
    });
    let cue = preview
        .retained
        .as_ref()
        .and_then(|r| r.descriptor.cues.get(index))
        .copied()
        .flatten();
    card.paint(
        world,
        camera,
        widgets,
        "demo",
        image_height + if demo.is_some() { 310. } else { 268. },
        demo.map_or("Lesson preview", |d| d.title),
        theme,
    )?;
    let mut model = InterfaceControl::button(
        "document/scripts",
        format!(
            "Lesson preview model: {}{caption}",
            if current { "" } else { "Preparing image — " }
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
        Some(if image.is_some() {
            ""
        } else {
            "Loading example…"
        }),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Model)),
        Node {
            border: UiRect::all(px(1.)),
            border_radius: BorderRadius::all(px(6.)),
            ..rect(x + 16., y + 72., image_width, image_height)
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
    let top = y + 80. + image_height;
    widgets.colored_text(
        world,
        camera,
        "scripts-preview-purpose",
        rect(x + 16., top, w - 72., 18.),
        "Drag to turn · Arrow keys · Home to fit",
        10.,
        61,
        theme.mute,
    );
    widgets.button(
        world,
        camera,
        "scripts-preview-fit",
        InterfaceControl::button("document/scripts", "Fit preview"),
        Some(""),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Fit)),
        rect(x + w - 40., top - 3., 24., 24.),
        Some(interface_shell::ribbon::Icon::Fit),
        61,
    )?;
    let extra = if let Some(demo) = demo {
        feature::guide(
            world,
            camera,
            widgets,
            (x + 16., top + 30., w - 32.),
            demo,
            cue,
            theme,
        );
        42.
    } else {
        0.
    };
    widgets.text(
        world,
        camera,
        "scripts-preview-caption",
        rect(x + 16., top + 30. + extra, w - 32., 44.),
        &caption,
        13.,
        61,
    );
    let buttons = top + 84. + extra;
    let bw = (w - 48.) / 3.;
    for (i, (label, action, disabled)) in [
        ("Previous", Action::Previous, !ready || index == 0),
        (
            if playing { "Pause" } else { "Replay" },
            if playing {
                Action::Stop
            } else {
                Action::Replay
            },
            count < 2,
        ),
        ("Next", Action::Next, !ready || index + 1 >= count),
    ]
    .into_iter()
    .enumerate()
    {
        let mut control = InterfaceControl::button("document/scripts", label);
        control.disabled = disabled;
        let entity = widgets.button(
            world,
            camera,
            &format!("scripts-preview-button-{i}"),
            control,
            Some(label),
            NativeCommand::File(FileCommand::ScriptPreview(action)),
            rect(x + 16. + i as f32 * (bw + 8.), buttons, bw, 32.),
            None,
            61,
        )?;
        if i == 1 {
            interface_shell::primary_button(world, entity);
        }
    }
    let footer = buttons + 44.;
    widgets.colored_text(
        world,
        camera,
        "scripts-preview-safety",
        rect(x + 16., footer, w - 144., 24.),
        "Your design stays unchanged",
        10.,
        61,
        theme.mute,
    );
    if demo.is_some() {
        widgets.button(
            world,
            camera,
            "extrude-open-help",
            InterfaceControl::button("document/scripts", "Open Extrude help"),
            Some("Open help"),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Help)),
            rect(x + w - 120., footer - 2., 104., 28.),
            Some(interface_shell::ribbon::Icon::Book),
            61,
        )?;
    }
    Ok(())
}
