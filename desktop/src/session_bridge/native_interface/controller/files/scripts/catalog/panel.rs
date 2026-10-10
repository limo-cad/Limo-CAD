use super::super::super::super::chrome::{self, rect};
use super::*;

pub(crate) fn paint_library(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    width: f32,
    height: f32,
    theme: ViewportUiTheme,
    busy: bool,
) -> Result<(), String> {
    let w = (width - 40.).clamp(280., 540.);
    let h = (height - 80.).clamp(330., 760.);
    let x = (width - w - 20.).max(4.);
    let y = 32.;
    let entries = examples();
    let per_page = ((h - 104.) / 108.).floor().max(1.) as usize;
    let pages = entries.len().div_ceil(per_page).max(1);
    let page = world.resource::<Files>().script.library.page.min(pages - 1);
    widgets.panel(
        world,
        camera,
        "scripts-card",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        60,
    );
    widgets.text(
        world,
        camera,
        "scripts-library-title",
        rect(x + 12., y + 8., w - 24., 24.),
        "Script examples",
        15.,
        61,
    );
    widgets.text(
        world,
        camera,
        "scripts-library-purpose",
        rect(x + 12., y + 36., w - 24., 30.),
        "Open authored source for review. Run explicitly creates a new design.",
        11.,
        61,
    );
    for (index, example) in entries
        .iter()
        .skip(page * per_page)
        .take(per_page)
        .enumerate()
    {
        let top = y + 72. + index as f32 * 108.;
        widgets.text(
            world,
            camera,
            &format!("scripts-example-group-{index}"),
            rect(x + 12., top, w - 24., 16.),
            example.group(),
            10.,
            61,
        );
        let mut control = InterfaceControl::button("document/scripts", &example.name);
        control.disabled = busy;
        control.selected = Some(
            world
                .resource::<Files>()
                .script
                .example
                .is_some_and(|current| current.id == example.id),
        );
        widgets.button(
            world,
            camera,
            &format!("scripts-example-{index}"),
            control,
            Some(&example.name),
            NativeCommand::File(FileCommand::OpenExample(example.id.clone())),
            rect(x + 12., top + 20., w - 24., 26.),
            None,
            61,
        )?;
        widgets.text(
            world,
            camera,
            &format!("scripts-example-summary-{index}"),
            Node {
                overflow: Overflow::clip(),
                ..rect(x + 12., top + 50., w - 24., 48.)
            },
            &example.summary,
            11.,
            61,
        );
    }
    let third = (w - 40.) / 3.;
    for (key, label, command, left, disabled) in [
        (
            "scripts-library-back",
            "Back to Scripts".to_owned(),
            FileCommand::BrowseExamples,
            x + 12.,
            false,
        ),
        (
            "scripts-library-previous",
            "Previous examples".to_owned(),
            FileCommand::ExamplePage(page.saturating_sub(1)),
            x + 20. + third,
            page == 0,
        ),
        (
            "scripts-library-next",
            format!("Next examples ({}/{pages})", page + 1),
            FileCommand::ExamplePage(page + 1),
            x + 28. + third * 2.,
            page + 1 >= pages,
        ),
    ] {
        let mut control = InterfaceControl::button("document/scripts", &label);
        control.disabled = disabled;
        widgets.button(
            world,
            camera,
            key,
            control,
            Some(&label),
            NativeCommand::File(command),
            rect(left, y + h - 36., third, 26.),
            None,
            61,
        )?;
    }
    Ok(())
}
