use super::super::super::super::chrome::{self, rect};
use super::*;

pub(crate) fn paint(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    _height: f32,
    theme: ViewportUiTheme,
) -> Result<(), String> {
    let w = (width - 40.).clamp(280., 420.);
    let x = (width - w - 20.).max(4.);
    let y = 32.;
    let image_width = (w - 24.).min(300.);
    let image_height = image_width * 176. / 300.;
    let image_x = x + (w - image_width) / 2.;
    let scale = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map_or(1., |window| window.scale_factor());
    let scale = (scale
        * world
            .get_resource::<bevy::ui::UiScale>()
            .map_or(1., |s| s.0))
    .clamp(1., 3.5);
    let size = [
        (image_width * scale).round() as u32,
        (image_height * scale).round() as u32,
    ];
    let preview = &mut world.resource_mut::<Files>().script.preview;
    if preview.size != size {
        preview.size = size;
        preview.changed()?;
    }
    let count = preview.count();
    let index = preview.index;
    let playing = preview.playing;
    let image = (preview.image_index == Some(index))
        .then(|| preview.image.clone())
        .flatten();
    let current_image = image.is_some() && preview.rendered == preview.revision;
    let caption = preview.error.clone().unwrap_or_else(|| {
        preview
            .retained
            .as_ref()
            .map(|retained| {
                format!(
                    "{}/{} — {}",
                    index + 1,
                    count,
                    retained.descriptor.captions[index]
                )
            })
            .unwrap_or_else(|| "Preparing the isolated lesson preview...".into())
    });
    let ready = preview.retained.is_some();
    widgets.panel(
        world,
        camera,
        "scripts-card",
        rect(x, y, w, image_height + 200.),
        theme.panel.with_alpha(1.),
        60,
    );
    widgets.text(
        world,
        camera,
        "scripts-preview-heading",
        rect(x + 12., y + 8., w - 24., 22.),
        "Lesson preview",
        15.,
        61,
    );
    widgets.text(
        world,
        camera,
        "scripts-preview-purpose",
        rect(x + 12., y + 34., w - 24., 24.),
        "Drag or use arrow keys to turn. Home fits the model.",
        11.,
        61,
    );
    let mut control = InterfaceControl::button(
        "document/scripts",
        format!(
            "Lesson preview model: {}{caption}",
            if current_image {
                ""
            } else {
                "Preparing image — "
            }
        ),
    );
    control.owned_keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home"]
        .map(limo_cad_interface::KeyChord::plain)
        .into();
    let mut bounds = rect(image_x, y + 64., image_width, image_height);
    bounds.border = UiRect::all(px(1.));
    let entity = widgets.button(
        world,
        camera,
        "scripts-preview-model",
        control,
        Some(if image.is_some() {
            ""
        } else {
            "Rendering preview..."
        }),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Model)),
        bounds,
        None,
        61,
    )?;
    if let Some(image) = image {
        world.entity_mut(entity).insert(ImageNode::new(image));
    } else {
        world.entity_mut(entity).remove::<ImageNode>();
    }
    widgets.text(
        world,
        camera,
        "scripts-preview-caption",
        Node {
            overflow: Overflow::clip(),
            ..rect(x + 12., y + 72. + image_height, w - 24., 46.)
        },
        &caption,
        11.,
        61,
    );
    let top = y + image_height + 124.;
    let fourth = (w - 42.) / 4.;
    for (i, (label, action, disabled)) in [
        (
            "Previous preview frame",
            Action::Previous,
            !ready || index == 0,
        ),
        (
            "Next preview frame",
            Action::Next,
            !ready || index + 1 >= count,
        ),
        (
            if playing {
                "Stop preview replay"
            } else {
                "Replay preview"
            },
            if playing {
                Action::Stop
            } else {
                Action::Replay
            },
            count < 2,
        ),
        ("Fit preview", Action::Fit, !ready),
    ]
    .into_iter()
    .enumerate()
    {
        let mut control = InterfaceControl::button("document/scripts", label);
        control.disabled = disabled;
        let caption = [
            "Previous",
            "Next",
            if playing { "Stop" } else { "Replay" },
            "Fit",
        ][i];
        widgets.button(
            world,
            camera,
            &format!("scripts-preview-button-{i}"),
            control,
            Some(caption),
            NativeCommand::File(FileCommand::ScriptPreview(action)),
            rect(x + 12. + i as f32 * (fourth + 6.), top, fourth, 26.),
            None,
            61,
        )?;
    }
    widgets.button(
        world,
        camera,
        "scripts-preview-close",
        InterfaceControl::button("document/scripts", "Back to script source"),
        Some("Back to script source"),
        NativeCommand::File(FileCommand::ScriptPreview(Action::Close)),
        rect(x + 12., top + 36., w - 24., 28.),
        None,
        61,
    )?;
    Ok(())
}
