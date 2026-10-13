use super::*;
use limo_cad_interface::{ChoiceOption, KeyChord};

type ViewRows = Vec<(String, Option<Field>, Option<Command>, Option<String>)>;

fn option(value: impl Into<String>, label: impl Into<String>) -> ChoiceOption {
    ChoiceOption {
        value: value.into(),
        label: label.into(),
        disabled: false,
    }
}

pub(crate) fn printer_choices() -> Vec<(String, String, PrintBedDto)> {
    limo_cad_core::embedded_printer_catalog()
        .profiles
        .iter()
        .flat_map(|profile| {
            [
                (
                    format!("{}:main", profile.id),
                    profile.main.name.clone(),
                    profile.main.clone(),
                ),
                (
                    format!("{}:dual", profile.id),
                    profile.dual.name.clone(),
                    profile.dual.clone(),
                ),
            ]
        })
        .collect()
}

pub(super) fn field_text(state: &State, field: Field) -> String {
    match field {
        Field::Saved => state.selected.clone().unwrap_or_default(),
        Field::Name => state
            .draft
            .as_ref()
            .map(|v| v.name.clone())
            .unwrap_or_default(),
        Field::Target => state.target.clone(),
        Field::Translation(i) => state
            .placement
            .as_ref()
            .map(|p| p.translation[i].text().into())
            .unwrap_or_default(),
        Field::Rotation(i) => state
            .placement
            .as_ref()
            .map(|p| p.rotation[i].text().into())
            .unwrap_or_default(),
        Field::PrintLayout => state
            .draft
            .as_ref()
            .is_some_and(|v| v.print_layout)
            .to_string(),
        Field::Printer => state
            .draft
            .as_ref()
            .and_then(|v| {
                printer_choices()
                    .into_iter()
                    .find(|(_, _, bed)| *bed == v.print_bed)
                    .map(|(key, _, _)| key)
            })
            .unwrap_or_else(|| "custom".into()),
        Field::VisibleBody => state
            .target
            .strip_prefix("body:")
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|id| {
                state
                    .draft
                    .as_ref()
                    .is_some_and(|v| v.visible_body_ids.contains(&id))
            })
            .to_string(),
    }
}

pub(super) fn choices(
    state: &State,
    engine: &AppState,
    field: Field,
) -> Result<Option<Vec<ChoiceOption>>, String> {
    Ok(match field {
        Field::Saved => Some(
            state
                .views
                .iter()
                .map(|v| {
                    option(
                        &v.name,
                        format!(
                            "{}{}",
                            v.name,
                            if v.print_layout {
                                " · print layout"
                            } else {
                                ""
                            }
                        ),
                    )
                })
                .collect(),
        ),
        Field::Target => {
            let structure = read_assembly(engine)?.component_structure;
            let mut options = Vec::new();
            fn append(
                options: &mut Vec<ChoiceOption>,
                structure: &limo_cad_sketch::ComponentStructureDto,
                parent: Option<limo_cad_sketch::OccurrenceId>,
                depth: usize,
            ) {
                if depth > 64 {
                    return;
                }
                for occurrence in structure
                    .occurrences
                    .iter()
                    .filter(|o| o.parent_occurrence_id == parent)
                {
                    options.push(option(
                        format!("occurrence:{}", occurrence.id.0),
                        format!(
                            "{}{} · occurrence {}",
                            "  ".repeat(depth),
                            occurrence.name,
                            occurrence.id.0
                        ),
                    ));
                    append(options, structure, Some(occurrence.id), depth + 1);
                }
            }
            append(&mut options, &structure, None, 0);
            options.extend(engine.solid_scene_snapshot().bodies.iter().map(|b| {
                option(
                    format!("body:{}", b.id.0),
                    format!("{} · all occurrences of body {}", b.name, b.id.0),
                )
            }));
            Some(options)
        }
        Field::PrintLayout => Some(vec![
            option("false", "Presentation view"),
            option("true", "Print layout"),
        ]),
        Field::VisibleBody => Some(vec![
            option("true", "Included in this view"),
            option("false", "Explicitly excluded from this view"),
        ]),
        Field::Printer => {
            let mut options: Vec<_> = printer_choices()
                .into_iter()
                .map(|(key, label, _)| option(key, label))
                .collect();
            if field_text(state, Field::Printer) == "custom" {
                let mut custom = option("custom", "Saved custom bed (retained)");
                custom.disabled = true;
                options.push(custom);
            }
            Some(options)
        }
        _ => None,
    })
}

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    engine: &AppState,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let w = 430_f32.min((width - 306.).max(210.));
    let x = (width - 164. - w).max(290.);
    let y = 132.;
    let h = (height - y - 108.).clamp(240., 540.);
    let theme = crate::native_viewport::ui::theme(world);
    workbench::card(
        (&mut state.widgets, world, camera),
        "named-views-card",
        chrome::rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        4.,
        48,
    );
    state.widgets.text(
        world,
        camera,
        "named-views-title",
        chrome::rect(x + 12., y + 8., w - 90., 20.),
        "Named views and print layouts",
        13.,
        50,
    );
    state.widgets.button(
        world,
        camera,
        "named-views-close",
        InterfaceControl::button("document/views", "Close named views"),
        Some("Close"),
        NativeCommand::NamedView(state.generation, Command::Close),
        chrome::rect(x + w - 64., y + 6., 52., 24.),
        None,
        50,
    )?;
    let mut rows: ViewRows = vec![
        ("Saved named view".into(), Some(Field::Saved), None, None),
        (
            "Create named view".into(),
            None,
            Some(Command::Create),
            None,
        ),
        ("View name".into(), Some(Field::Name), None, None),
        (
            "Capture current camera and visibility".into(),
            None,
            Some(Command::Capture),
            None,
        ),
        ("View purpose".into(), Some(Field::PrintLayout), None, None),
        ("Printer bed".into(), Some(Field::Printer), None, None),
        (
            "Part print settings".into(),
            None,
            Some(Command::PrintSettings),
            None,
        ),
        (
            "CAD occurrence or body".into(),
            Some(Field::Target),
            None,
            None,
        ),
    ];
    if !state.target.is_empty() {
        for (axis, letter) in ["X", "Y", "Z"].into_iter().enumerate() {
            rows.push((
                format!("View offset {letter} ({:?})", engine.document_units()),
                Some(Field::Translation(axis)),
                None,
                None,
            ));
        }
        if state.target.starts_with("occurrence:") {
            for (axis, letter) in ["X", "Y", "Z"].into_iter().enumerate() {
                rows.push((
                    format!("View rotation {letter} (degrees)"),
                    Some(Field::Rotation(axis)),
                    None,
                    None,
                ));
            }
        } else {
            rows.push((
                "Body inclusion (all its occurrences)".into(),
                Some(Field::VisibleBody),
                None,
                None,
            ));
        }
    }
    rows.extend([
        ("Preview draft".into(), None, Some(Command::Preview), None),
        (
            "Check print layout".into(),
            None,
            Some(Command::Check),
            None,
        ),
        (
            "Apply proposed group corrections to draft".into(),
            None,
            Some(Command::ApplyCorrections),
            None,
        ),
        ("Save view".into(), None, Some(Command::Save), None),
        (
            "Recall saved view".into(),
            None,
            Some(Command::Recall),
            None,
        ),
        (
            "Rename saved view".into(),
            None,
            Some(Command::Rename),
            None,
        ),
        (
            "Delete saved view".into(),
            None,
            Some(Command::Delete),
            None,
        ),
        (
            "Reset to assembled placement".into(),
            None,
            Some(Command::Reset),
            None,
        ),
    ]);
    if let Some(report) = &state.report {
        rows.push((
            "Layout diagnostic summary".into(),
            None,
            None,
            Some(format!(
                "{} instances · {} multipart groups · {} excluded",
                report["printable_instances"],
                report["printable_groups"],
                report["excluded_instances"]
            )),
        ));
        if let Some(issues) = report["issues"].as_array() {
            if issues.is_empty() {
                rows.push((
                    "Layout check".into(),
                    None,
                    None,
                    Some("No layout issues found".into()),
                ));
            }
            for (i, issue) in issues.iter().enumerate() {
                rows.push((
                    format!(
                        "Layout issue {} · {}",
                        i + 1,
                        issue["code"].as_str().unwrap_or("")
                    ),
                    None,
                    None,
                    Some(issue_message(engine, issue)?),
                ));
            }
        }
        rows.push(("Proposed correction".into(), None, None, Some(if report["proposal_fits"] == true {
            "Whole CAD groups fit after the proposed translations; rotations and part alignment are retained."
        } else { "The proposed arrangement does not fit. Review group size or bed choice." }.into())));
    }
    let per_page = (((h - 88.) / 51.).floor() as usize).clamp(1, 8);
    state.scroll = state.scroll.min(rows.len().saturating_sub(per_page));
    let total = rows.len();
    for (index, (label, field, command, information)) in rows
        .into_iter()
        .skip(state.scroll)
        .take(per_page)
        .enumerate()
    {
        let ry = y + 36. + index as f32 * 51.;
        state.widgets.text(
            world,
            camera,
            &format!("view-label-{label}"),
            chrome::rect(x + 12., ry, w - 24., 16.),
            &label,
            10.,
            50,
        );
        let mut control = InterfaceControl::button("document/views", &label);
        control.disabled = worker::busy(world)
            || command
                .as_ref()
                .is_some_and(|command| saved_action_error(state, command).is_some());
        let options = if let Some(field) = field {
            choices(state, engine, field)?
        } else {
            None
        };
        let mut caption = None;
        if let Some(options) = options {
            let value = field_text(state, field.unwrap());
            caption = options
                .iter()
                .find(|o| o.value == value)
                .map(|o| o.label.clone());
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
            control.field = ControlField::Choice { value, options };
        } else if let Some(field) = field {
            control.owned_keys = vec![KeyChord::plain("Enter"), KeyChord::plain("Escape")];
            control.field = ControlField::Text {
                value: field_text(state, field),
                read_only: false,
                selection: None,
            };
        } else if let Some(information) = information {
            control.field = ControlField::Text {
                value: information,
                read_only: true,
                selection: None,
            };
        }
        if command == Some(Command::ApplyCorrections) {
            control.disabled |= state.report.as_ref().is_none_or(|r| {
                r["proposal_fits"] != true
                    || r["proposed_translations"]
                        .as_array()
                        .is_none_or(Vec::is_empty)
            });
        }
        state.widgets.button(
            world,
            camera,
            &format!("named-view-{label}"),
            control,
            caption.as_deref(),
            NativeCommand::NamedView(
                state.generation,
                field
                    .map(Command::Field)
                    .or(command)
                    .unwrap_or(Command::Info),
            ),
            chrome::rect(x + 12., ry + 17., w - 24., 28.),
            None,
            50,
        )?;
    }
    for (label, caption, command, offset, disabled) in [
        (
            "Previous named-view fields",
            "↑",
            Command::Scroll(-(per_page as i32)),
            12.,
            state.scroll == 0,
        ),
        (
            "More named-view fields",
            "↓",
            Command::Scroll(per_page as i32),
            43.,
            state.scroll + per_page >= total,
        ),
    ] {
        let mut control = InterfaceControl::button("document/views", label);
        control.disabled = disabled || worker::busy(world);
        state.widgets.button(
            world,
            camera,
            label,
            control,
            Some(caption),
            NativeCommand::NamedView(state.generation, command),
            chrome::rect(x + offset, y + h - 32., 27., 24.),
            None,
            50,
        )?;
    }
    let message = state.error.as_deref().unwrap_or(if state.previewing {
        "Draft preview · save and recall before exporting"
    } else if state.selected.is_none() {
        "New draft · Save before recall, rename or delete"
    } else {
        "Saved views export whether or not marked for printing"
    });
    state.widgets.text(
        world,
        camera,
        "named-view-status",
        chrome::rect(x + 84., y + h - 34., w - 96., 28.),
        message,
        10.,
        50,
    );
    Ok(())
}
