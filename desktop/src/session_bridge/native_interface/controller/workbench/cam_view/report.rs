//! Readable, paged output of the shared CAM verifier.
use super::*;

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    width: f32,
    side: f32,
) -> Result<(), String> {
    let height = interface_shell::window_ui_size(world).map_or(860., |size| size.y);
    let w = (width - side - 28.).clamp(260., 540.);
    let h = (height - 164.).clamp(150., 720.);
    let x = (width - w - 14.).max(4.);
    let y = 132.;
    let theme = crate::native_viewport::ui::theme(world);
    super::super::card(
        (&mut state.widgets, world, camera),
        "cam-report-card",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        6.,
        80,
    );
    state.widgets.text(
        world,
        camera,
        "cam-report-title",
        rect(x + 12., y + 10., w - 24., 24.),
        "CAM report",
        16.,
        82,
    );
    let details = if !state.error.is_empty() {
        state.error.as_str()
    } else {
        state
            .prepared
            .as_ref()
            .map_or("No CAM result", |p| p.details.as_str())
    };
    let columns = ((w - 28.) / 6.8).floor().max(1.) as usize;
    let mut lines = Vec::new();
    for line in details.lines() {
        let mut current = String::new();
        for word in line.split_whitespace() {
            if !current.is_empty() && current.chars().count() + word.chars().count() + 1 > columns {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        lines.push(current);
    }
    let per_page = (((h - 92.) / 17.).floor() as usize).max(1);
    let pages = lines.len().max(1).div_ceil(per_page);
    state.report_page = state.report_page.min(pages - 1);
    let text = lines
        .into_iter()
        .skip(state.report_page * per_page)
        .take(per_page)
        .collect::<Vec<_>>()
        .join("\n");
    state.widgets.text(
        world,
        camera,
        "cam-report-text",
        rect(x + 12., y + 42., w - 24., h - 90.),
        &text,
        11.,
        82,
    );
    for (index, (label, command, disabled)) in [
        (
            "Previous page",
            Command::ReportPage(-1),
            state.report_page == 0,
        ),
        (
            "Next page",
            Command::ReportPage(1),
            state.report_page + 1 >= pages,
        ),
        ("Close report", Command::CloseReport, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut control = InterfaceControl::button("cam-report", label);
        control.modal_scope = Some("cam-report".into());
        control.disabled = disabled;
        state.widgets.button(
            world,
            camera,
            &format!("cam-report-{index}"),
            control,
            Some(label),
            NativeCommand::Workbench(super::super::Command::CamView(command)),
            rect(
                x + 12. + index as f32 * (w - 24.) / 3.,
                y + h - 38.,
                (w - 30.) / 3.,
                26.,
            ),
            None,
            82,
        )?;
    }
    state.widgets.text(
        world,
        camera,
        "cam-report-page",
        rect(x + w - 140., y + 12., 125., 20.),
        &format!("{} / {pages}", state.report_page + 1),
        11.,
        82,
    );
    Ok(())
}
