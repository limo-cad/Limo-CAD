use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn button(
    world: &mut World,
    camera: Entity,
    widgets: &mut Widgets,
    key: &str,
    label: &str,
    caption: Option<&str>,
    command: Command,
    bounds: Node,
    disabled: bool,
    selected: Option<bool>,
) -> Result<(), String> {
    let mut control = InterfaceControl::button("cam-library", label);
    control.modal_scope = Some("cam-library".into());
    control.disabled = disabled;
    control.selected = selected;
    widgets.button(
        world,
        camera,
        key,
        control,
        caption,
        NativeCommand::Cam(super::super::Command::Central(command)),
        bounds,
        None,
        84,
    )?;
    Ok(())
}

pub(super) fn wrapped(text: &str, columns: usize, max_lines: usize) -> String {
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
            for (index, chunk) in chars.chunks(columns).enumerate() {
                if index > 0 {
                    lines.push(std::mem::take(&mut current));
                    length = 0;
                }
                if !current.is_empty() {
                    current.push(' ');
                    length += 1;
                }
                current.extend(chunk);
                length += chunk.len();
            }
        }
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            last.push('…');
        }
    }
    lines.join("\n")
}

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let theme = crate::native_viewport::ui::theme(world);
    let w = (width - 32.).clamp(480., 960.);
    let h = (height - 48.).clamp(340., 720.);
    let x = (width - w) / 2.;
    let y = (height - h) / 2.;
    let list_w = (w * 0.30).clamp(160., 270.);
    let detail_x = x + list_w + 22.;
    let detail_w = w - list_w - 38.;
    let busy = worker::busy(world) || storage::awaiting(state);
    let dirty = dirty(state);
    let chosen = selected_id(state);
    state.widgets.panel(
        world,
        camera,
        "library-shade",
        rect(0., 30., width, height - 30.),
        Color::srgba(0., 0., 0., 0.42),
        80,
    );
    state.widgets.panel(
        world,
        camera,
        "library-panel",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        82,
    );
    state.widgets.text(
        world,
        camera,
        "library-title",
        rect(x + 16., y + 12., w - 120., 22.),
        "Central tool library",
        16.,
        84,
    );

    button(
        world,
        camera,
        &mut state.widgets,
        "library-close",
        "Close library",
        Some("Close"),
        Command::Close,
        rect(x + w - 88., y + 10., 72., 28.),
        busy || dirty,
        None,
    )?;
    let path = state
        .snapshot
        .as_ref()
        .map_or("Loading the configured collection…", |snapshot| {
            snapshot.path.as_str()
        });
    state.widgets.text(
        world,
        camera,
        "library-path",
        rect(x + 16., y + 40., w - 32., 32.),
        &wrapped(path, ((w - 32.) / 6.).floor() as usize, 2),
        10.,
        84,
    );

    if state.storage.is_some() {
        return storage::paint(world, camera, state, x, y, w, h);
    }

    let list_x = x + 14.;
    let top = y + 80.;
    for (index, (label, command)) in [
        ("New tool", Command::New),
        ("Copy tool", Command::Duplicate),
        ("Delete tool", Command::Delete),
    ]
    .into_iter()
    .enumerate()
    {
        let bw = (list_w - 8.) / 3.;
        button(
            world,
            camera,
            &mut state.widgets,
            &format!("library-list-action-{index}"),
            label,
            Some(match command {
                Command::New => "New",
                Command::Duplicate => "Copy",
                _ => "Delete",
            }),
            command,
            rect(list_x + index as f32 * (bw + 4.), top, bw, 28.),
            busy || dirty || state.snapshot.is_none() || (index > 0 && chosen.is_none()),
            None,
        )?;
    }
    let count = state
        .snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.tools.len());
    let list_size = (((h - 284.) / 39.).floor() as usize).clamp(1, 5);
    if state.list_size != list_size {
        state.page = state
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                chosen.and_then(|id| snapshot.tools.iter().position(|tool| tool.id == id))
            })
            .unwrap_or(0)
            / list_size;
        state.list_size = list_size;
    }
    state.page = state.page.min(count.saturating_sub(1) / list_size);
    if let Some(snapshot) = &state.snapshot {
        for (index, tool) in snapshot
            .tools
            .iter()
            .enumerate()
            .skip(state.page * list_size)
            .take(list_size)
        {
            let label = tool.number.map_or_else(
                || tool.name.clone(),
                |number| format!("T{number} · {}", tool.name),
            );
            button(
                world,
                camera,
                &mut state.widgets,
                &format!("library-tool-{}", tool.id),
                &label,
                None,
                Command::Select(tool.id),
                rect(
                    list_x,
                    top + 38. + (index % list_size) as f32 * 39.,
                    list_w,
                    34.,
                ),
                busy || dirty,
                Some(chosen == Some(tool.id) && !state.creating),
            )?;
        }
        if snapshot.tools.is_empty() {
            state.widgets.text(
                world,
                camera,
                "library-empty",
                rect(list_x + 6., top + 42., list_w - 12., 48.),
                "No central tools. Create a tool or publish a project snapshot.",
                12.,
                84,
            );
        }
    }
    for (index, (label, delta, disabled)) in [
        ("Previous tools", -1, state.page == 0),
        ("More tools", 1, (state.page + 1) * list_size >= count),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            world,
            camera,
            &mut state.widgets,
            &format!("library-list-page-{index}"),
            label,
            Some(if index == 0 { "Previous" } else { "Next" }),
            Command::Page(delta),
            rect(
                list_x + index as f32 * (list_w + 4.) / 2.,
                top + 44. + list_size as f32 * 39.,
                (list_w - 4.) / 2.,
                28.,
            ),
            busy || disabled,
            None,
        )?;
    }

    if let Some(draft) = state.draft.as_ref() {
        let fields = draft
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| {
                tool::visible(draft, &field.path) && presets::visible_tool(draft, &field.path)
            })
            .collect::<Vec<_>>();
        let page_size = (((h - 254.) / 47.).floor() as usize).clamp(1, 8);
        state.field_page = state
            .field_page
            .min(fields.len().saturating_sub(1) / page_size);
        for (row, (index, field)) in fields
            .iter()
            .copied()
            .enumerate()
            .skip(state.field_page * page_size)
            .take(page_size)
        {
            let fy = top + (row % page_size) as f32 * 47.;
            state.widgets.text(
                world,
                camera,
                &format!("library-field-label-{index}"),
                rect(detail_x, fy, detail_w, 16.),
                &field.label,
                11.,
                84,
            );
            let mut control = InterfaceControl::button("cam-library", &field.label);
            control.modal_scope = Some("cam-library".into());
            control.disabled = busy;
            let caption = if let Some(options) = &field.options {
                let caption = options
                    .iter()
                    .find(|option| option.value == field.text)
                    .map(|option| option.label.clone())
                    .unwrap_or("Choose…".into());
                control.role = "combobox".into();
                control.disabled |= options.is_empty();
                control.owned_keys = [
                    "ArrowUp",
                    "ArrowDown",
                    "ArrowLeft",
                    "ArrowRight",
                    "Home",
                    "End",
                ]
                .map(KeyChord::plain)
                .into();
                control.field = Field::Choice {
                    value: field.text.clone(),
                    options: options.clone(),
                };
                Some(caption)
            } else {
                control.field = Field::Text {
                    value: field.text.clone(),
                    read_only: false,
                    selection: None,
                };
                None
            };
            state.widgets.button(
                world,
                camera,
                &format!("library-field-{}", field.path),
                control,
                caption.as_deref(),
                NativeCommand::Cam(super::super::Command::Central(Command::Edit(index))),
                rect(detail_x, fy + 17., detail_w, 27.),
                None,
                84,
            )?;
        }
        let fy = top + page_size as f32 * 47. + 3.;
        if fields.len() > page_size {
            for (index, (label, delta, disabled)) in [
                ("Previous library fields", -1, state.field_page == 0),
                (
                    "More library fields",
                    1,
                    (state.field_page + 1) * page_size >= fields.len(),
                ),
            ]
            .into_iter()
            .enumerate()
            {
                button(
                    world,
                    camera,
                    &mut state.widgets,
                    &format!("library-fields-page-{index}"),
                    label,
                    Some(if index == 0 {
                        "Previous fields"
                    } else {
                        "More fields"
                    }),
                    Command::Fields(delta),
                    rect(
                        detail_x + index as f32 * (detail_w + 4.) / 2.,
                        fy,
                        (detail_w - 4.) / 2.,
                        26.,
                    ),
                    busy || disabled,
                    None,
                )?;
            }
        }
        if presets::editing(draft) {
            let selected = !draft
                .fields
                .iter()
                .find(|field| field.path == "/native/ui/cutting_profile")
                .is_none_or(|field| field.text.is_empty());
            for (index, (label, action)) in [
                ("Add central preset", presets::Command::Add),
                ("Copy central preset", presets::Command::Duplicate),
                ("Remove central preset", presets::Command::Remove),
            ]
            .into_iter()
            .enumerate()
            {
                let caption = match action {
                    presets::Command::Add => "Add preset",
                    presets::Command::Duplicate => "Copy preset",
                    presets::Command::Remove => "Remove preset",
                };
                let bw = (detail_w - 8.) / 3.;
                button(
                    world,
                    camera,
                    &mut state.widgets,
                    &format!("library-preset-action-{index}"),
                    label,
                    Some(caption),
                    Command::Preset(action),
                    rect(detail_x + index as f32 * (bw + 4.), fy + 30., bw, 27.),
                    busy || (action != presets::Command::Add && !selected),
                    None,
                )?;
            }
        }
        let bw = (detail_w - 8.) / 3.;
        for (index, (label, caption, command, disabled)) in [
            (
                if state.creating {
                    "Create central tool"
                } else {
                    "Apply central tool"
                },
                if state.creating { "Create" } else { "Apply" },
                Command::Apply,
                false,
            ),
            (
                "Reset central tool",
                if state.creating {
                    "Cancel edit"
                } else {
                    "Reset"
                },
                Command::Reset,
                false,
            ),
            (
                "Import central tool",
                if chosen.is_some_and(|id| state.project.tool(id).is_some()) {
                    "Update project"
                } else {
                    "Add to project"
                },
                Command::Import,
                dirty || state.creating || chosen.is_none(),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            button(
                world,
                camera,
                &mut state.widgets,
                &format!("library-commit-{index}"),
                label,
                Some(caption),
                command,
                rect(detail_x + index as f32 * (bw + 4.), y + h - 82., bw, 29.),
                busy || disabled,
                None,
            )?;
        }
    }
    let summary = if !state.error.is_empty() {
        &state.error
    } else {
        &state.status
    };
    state.widgets.text(
        world,
        camera,
        "library-status",
        rect(list_x, y + h - 138., list_w, 48.),
        &wrapped(summary, (list_w / 6.).floor() as usize, 4),
        10.,
        84,
    );
    button(
        world,
        camera,
        &mut state.widgets,
        "library-refresh",
        "Refresh central library",
        Some("Refresh"),
        Command::Refresh,
        rect(list_x, y + h - 82., (list_w - 4.) / 2., 29.),
        busy || dirty,
        None,
    )?;
    button(
        world,
        camera,
        &mut state.widgets,
        "library-storage-open",
        "Library storage",
        Some("Storage"),
        Command::Storage(storage::Command::Open),
        rect(
            list_x + (list_w + 4.) / 2.,
            y + h - 82.,
            (list_w - 4.) / 2.,
            29.,
        ),
        busy || dirty,
        None,
    )?;
    let publish_label = state
        .project_tool
        .and_then(|id| state.project.tool(id))
        .map(|tool| format!("Publish project tool: {}", tool.name));
    if let Some(label) = publish_label {
        button(
            world,
            camera,
            &mut state.widgets,
            "library-publish",
            &label,
            Some("Publish project tool to central"),
            Command::Publish,
            rect(x + 14., y + h - 42., w - 28., 28.),
            busy || dirty || state.snapshot.is_none(),
            None,
        )?;
    } else {
        state.widgets.text(world,camera,"library-scope",rect(x+16.,y+h-38.,w-32.,24.),"Central changes leave project tools unchanged. Add or update a project snapshot explicitly.",11.,84);
    }
    Ok(())
}
