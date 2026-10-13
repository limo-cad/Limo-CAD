use super::*;

fn button(
    (world, camera, widgets): (&mut World, Entity, &mut Widgets),
    key: &str,
    label: &str,
    command: Command,
    bounds: Node,
    disabled: bool,
    selected: Option<bool>,
) -> Result<(), String> {
    let mut control = InterfaceControl::button("cam-export", label);
    control.modal_scope = Some("cam-export".into());
    control.disabled = disabled;
    control.selected = selected;
    widgets.button(
        world,
        camera,
        key,
        control,
        Some(label),
        NativeCommand::Workbench(super::super::Command::CamExport(command)),
        bounds,
        None,
        82,
    )?;
    Ok(())
}
fn field(
    (world, camera, widgets): (&mut World, Entity, &mut Widgets),
    label: &str,
    value: &str,
    command: Command,
    (x, y, width): (f32, f32, f32),
    disabled: bool,
    choice: bool,
) -> Result<(), String> {
    widgets.text(
        world,
        camera,
        &format!("post-label-{label}"),
        rect(x, y, width, 16.),
        label,
        11.,
        82,
    );
    let mut control = InterfaceControl::button("cam-export", label);
    control.modal_scope = Some("cam-export".into());
    control.disabled = disabled;
    let caption = if choice {
        control.role = "combobox".into();
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
            value: value.into(),
            options: [("true", "Yes"), ("false", "No")]
                .into_iter()
                .map(|(value, label)| ChoiceOption {
                    value: value.into(),
                    label: label.into(),
                    disabled: false,
                })
                .collect(),
        };
        Some(if value == "true" { "Yes" } else { "No" })
    } else {
        control.field = Field::Text {
            value: value.into(),
            read_only: false,
            selection: None,
        };
        None
    };
    widgets.button(
        world,
        camera,
        &format!("post-field-{label}"),
        control,
        caption,
        NativeCommand::Workbench(super::super::Command::CamExport(command)),
        rect(x, y + 17., width, 27.),
        None,
        82,
    )?;
    Ok(())
}
fn wrap(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(1);
    let mut lines = Vec::new();
    for line in text.lines() {
        let mut current = String::new();
        let mut length = 0;
        for word in line.split_whitespace() {
            let chars: Vec<_> = word.chars().collect();
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
pub(super) fn review_text(state: &State) -> String {
    let draft = state.draft.as_ref().unwrap();
    if let Some(prepared) = &state.prepared {
        let mut text = format!("{}\nOutput: {}\n\n", prepared.summary, prepared.file_name);
        if prepared.warnings.is_empty() {
            text.push_str("No post warnings were returned.\n");
        }
        for (i, warning) in prepared.warnings.iter().enumerate() {
            text.push_str(&format!("Warning {}: {warning}\n\n", i + 1));
        }
        if draft.kind == Kind::Nc {
            text.push_str("NC preview (first 20 lines):\n");
            text.push_str(
                &String::from_utf8_lossy(&prepared.bytes)
                    .lines()
                    .take(20)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        text
    } else {
        let post = &draft.post;
        let coordinate = |value: Option<f64>| {
            value.map_or_else(
                || "Not set".into(),
                |v| format!("{} {}", draft.units.from_mm(v), draft.units.length_label()),
            )
        };
        let mut text = format!(
            "Setup: {}\nMachine: {}\nPost: {:?}\nTool calls: {:?}\nMachine retract Z: {}\n",
            draft.setup_name,
            draft
                .machine_name
                .as_deref()
                .unwrap_or("No machine selected"),
            post.dialect,
            post.tool_call_mode,
            coordinate(post.machine_retract_z)
        );
        text.push_str(&draft.setup_summary);
        if let Some(siemens) = &post.siemens_828d {
            text.push_str(&format!("Tool changer: {:?}\nTool-change positioning: {:?}\nSUPA retract Z: {}\nStation X: {}\nStation Y: {}\nTool length offset: D{}\nOptional tool-change stop: {}\nPreload next tool: {}\nSpindle-stop subprogram: {}\n",
                siemens.atc_style, siemens.tool_change_positioning, coordinate(Some(siemens.supa_retract_z)), coordinate(siemens.station_x), coordinate(siemens.station_y),
                siemens.tool_length_offset, siemens.optional_stop_on_tool_change, siemens.preload_next_tool, siemens.spindle_stop_subprogram.as_deref().unwrap_or("None")));
        }
        text.push_str("\nChange machine-specific settings in the setup. Preparing output checks current toolpaths, stock and model through the shared CAM verifier.");
        text
    }
}

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    width: f32,
    height: f32,
    side: f32,
) -> Result<(), String> {
    let draft = state.draft.as_ref().unwrap();
    let w = (width - side - 28.).clamp(270., 440.);
    let h = (height - 180.).clamp(320., 740.);
    let x = (width - w - 14.).max(4.);
    let y = 150.;
    let theme = crate::native_viewport::ui::theme(world);
    super::super::card(
        (&mut state.widgets, world, camera),
        "cam-post-card",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        6.,
        80,
    );
    let title = if draft.kind == Kind::Nc {
        "Post NC"
    } else {
        "Export post events"
    };
    state.widgets.text(
        world,
        camera,
        "cam-post-title",
        rect(x + 12., y + 9., w - 24., 24.),
        title,
        16.,
        82,
    );
    let busy = state.preparing || state.saving || state.picker.is_some();
    let prepared = state.prepared.is_some();
    let mut content_y = y + 40.;
    if !prepared && draft.kind == Kind::Nc {
        field(
            (world, camera, &mut state.widgets),
            "Program name",
            &draft.program_name,
            Command::Name,
            (x + 12., content_y, w - 24.),
            busy,
            false,
        )?;
        content_y += 48.;
        field(
            (world, camera, &mut state.widgets),
            "Program number (optional)",
            &draft.program_number,
            Command::ProgramNumber,
            (x + 12., content_y, (w - 30.) * 0.65),
            busy,
            false,
        )?;
        field(
            (world, camera, &mut state.widgets),
            "Sequence numbers",
            if draft.sequence_numbers {
                "true"
            } else {
                "false"
            },
            Command::SequenceNumbers,
            (x + 18. + (w - 30.) * 0.65, content_y, (w - 30.) * 0.35),
            busy,
            true,
        )?;
        content_y += 52.;
    }
    let footer = y + h - 132.;
    let text = review_text(state);
    let lines = wrap(&text, ((w - 28.) / 6.8) as usize);
    let per_page = (((footer - content_y - 30.) / 17.).floor() as usize).max(1);
    let pages = lines.len().max(1).div_ceil(per_page);
    state.page = state.page.min(pages - 1);
    let page_text = lines
        .iter()
        .skip(state.page * per_page)
        .take(per_page)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    state.widgets.text(
        world,
        camera,
        "cam-post-review",
        rect(
            x + 12.,
            content_y,
            w - 24.,
            (per_page as f32 * 17.).max(17.),
        ),
        &page_text,
        11.,
        82,
    );
    let nav_y = footer - 29.;
    let nav_width = (w - 84.) / 2.;
    button(
        (world, camera, &mut state.widgets),
        "cam-post-prev",
        "Previous page",
        Command::Page(-1),
        rect(x + 12., nav_y, nav_width, 25.),
        state.page == 0 || busy,
        None,
    )?;
    state.widgets.text(
        world,
        camera,
        "cam-post-page",
        rect(x + 16. + nav_width, nav_y, 52., 25.),
        &format!("{} / {pages}", state.page + 1),
        11.,
        82,
    );
    button(
        (world, camera, &mut state.widgets),
        "cam-post-next",
        "Next page",
        Command::Page(1),
        rect(x + w - nav_width - 12., nav_y, nav_width, 25.),
        state.page + 1 >= pages || busy,
        None,
    )?;
    let (review_label, review_command, selected) = if prepared {
        ("Back to settings", Command::BackToSettings, None)
    } else {
        (
            "Reviewed setup and machine settings",
            Command::ReviewMachine,
            Some(state.draft.as_ref().unwrap().reviewed_machine),
        )
    };
    button(
        (world, camera, &mut state.widgets),
        "cam-post-reviewed",
        review_label,
        review_command,
        rect(x + 12., footer, w - 24., 27.),
        busy,
        selected,
    )?;
    let message = if !state.error.is_empty() {
        &state.error
    } else if state.preparing {
        "Preparing and verifying output…"
    } else if state.saving {
        "Saving reviewed output…"
    } else if state.picker.is_some() {
        "Choose the output destination…"
    } else {
        &state.status
    };
    state.widgets.text(
        world,
        camera,
        "cam-post-status",
        rect(x + 12., footer + 32., w - 24., 49.),
        message,
        11.,
        82,
    );
    button(
        (world, camera, &mut state.widgets),
        "cam-post-submit",
        if prepared {
            if state.draft.as_ref().unwrap().kind == Kind::Nc {
                "Save NC…"
            } else {
                "Save post events…"
            }
        } else {
            "Prepare and verify"
        },
        if prepared {
            Command::Save
        } else {
            Command::Prepare
        },
        rect(x + 12., y + h - 39., w - 116., 28.),
        busy || (!prepared && !state.draft.as_ref().unwrap().reviewed_machine),
        None,
    )?;
    button(
        (world, camera, &mut state.widgets),
        "cam-post-close",
        "Close Post",
        Command::Close,
        rect(x + w - 96., y + h - 39., 84., 28.),
        state.saving,
        None,
    )?;
    Ok(())
}
