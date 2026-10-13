use super::*;

fn wrap(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(1);
    let mut lines = Vec::new();
    for line in text.lines() {
        let mut current = String::new();
        let mut length = 0;
        for word in line.split_whitespace() {
            let chars = word.chars().collect::<Vec<_>>();
            if !current.is_empty() && length + 1 + chars.len() > columns {
                lines.push(std::mem::take(&mut current));
                length = 0;
            }
            if chars.len() > columns {
                for part in chars.chunks(columns) {
                    if !current.is_empty() {
                        lines.push(std::mem::take(&mut current));
                    }
                    current.extend(part);
                    length = part.len();
                }
            } else {
                if !current.is_empty() {
                    current.push(' ');
                    length += 1;
                }
                current.push_str(word);
                length += chars.len();
            }
        }
        lines.push(current);
    }
    lines
}

/// The parent owns the retained widget lifetime and normal command routing.
pub(in super::super::super) fn paint(
    world: &mut World,
    camera: Entity,
    widgets: &mut Widgets,
    (x, y, width, height): (f32, f32, f32, f32),
    command: impl Fn(Command) -> NativeCommand,
) -> Result<(), String> {
    world.init_resource::<State>();
    let pending = busy(world);
    let mut button =
        |world: &mut World, key: &str, label: &str, action: Command, bounds: Node, disabled| {
            let mut control = InterfaceControl::button("cam/private-posts", label);
            control.disabled = disabled;
            widgets.button(
                world,
                camera,
                key,
                control,
                Some(label),
                command(action),
                bounds,
                None,
                46,
            )
        };
    let half = (width - 6.) / 2.;
    button(
        world,
        "private-post-import",
        "Import post…",
        Command::Import,
        rect(x, y, half, 28.),
        pending,
    )?;
    button(
        world,
        "private-post-folder",
        "Open folder",
        Command::OpenFolder,
        rect(x + half + 6., y, half, 28.),
        pending,
    )?;
    button(
        world,
        "private-post-refresh",
        "Refresh posts",
        Command::Refresh,
        rect(x, y + 34., width, 28.),
        pending,
    )?;
    let lines = wrap(
        &caption(world),
        ((width - 8.) / 6.8).floor().max(1.) as usize,
    );
    let per_page = (((height - 104.) / 17.).floor() as usize).max(1);
    let pages = lines.len().max(1).div_ceil(per_page);
    let page = {
        let mut state = world.resource_mut::<State>();
        state.page = state.page.min(pages - 1);
        state.page
    };
    let nav_y = y + height - 28.;
    let nav_width = (width - 58.) / 2.;
    button(
        world,
        "private-post-previous",
        "Previous",
        Command::Page(-1),
        rect(x, nav_y, nav_width, 28.),
        page == 0,
    )?;
    button(
        world,
        "private-post-next",
        "Next",
        Command::Page(1),
        rect(x + width - nav_width, nav_y, nav_width, 28.),
        page + 1 >= pages,
    )?;
    let text = lines
        .iter()
        .skip(page * per_page)
        .take(per_page)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    widgets.text(
        world,
        camera,
        "private-post-diagnostics",
        rect(x + 4., y + 70., width - 8., per_page as f32 * 17.),
        &text,
        11.,
        46,
    );
    widgets.text(
        world,
        camera,
        "private-post-page",
        rect(x + nav_width + 3., nav_y + 6., 52., 20.),
        &format!("{} / {pages}", page + 1),
        11.,
        46,
    );
    Ok(())
}
