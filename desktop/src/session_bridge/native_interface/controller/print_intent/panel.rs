use super::*;

type PrintRows = Vec<(String, Option<Field>, Option<Command>, Option<String>)>;

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    width: f32,
    height: f32,
) -> Result<(), String> {
    modifiers::paint_labels(world, camera, state);
    let w = 470.;
    let x = (width - 156. - w).max(284.);
    let y = 132.;
    let h = (height - y - 108.).clamp(260., 550.);
    let per_page = (((h - 124.) / 51.).floor() as usize).clamp(1, 8);
    {
        let fill = native_viewport::ui::theme(world).panel.with_alpha(1.);
        workbench::card(
            (&mut state.widgets, world, camera),
            "print-intent-card",
            chrome::rect(x, y, w, h),
            fill,
            4.,
            49,
        )
    };
    state.widgets.text(
        world,
        camera,
        "print-intent-title",
        chrome::rect(x + 12., y + 8., w - 24., 18.),
        "Print Settings · requested",
        13.,
        51,
    );
    state.widgets.text(
        world,
        camera,
        "print-intent-inherit",
        chrome::rect(x + 12., y + 29., w - 24., 18.),
        if state.height_scope && state.height_editor.profile {
            "Variable layers are explicit. Samples use object-bottom Z; all repeats are bound."
        } else {
            "Blank = Inherit. 0 is explicit. All intentional repeats share part settings."
        },
        10.,
        51,
    );
    let mut rows: PrintRows = vec![
        ("Settings scope".into(), Some(Field::Scope), None, None),
        ("Print settings part".into(), Some(Field::Body), None, None),
        ("Requested walls".into(), Some(Field::Walls), None, None),
        (
            "Requested infill (%)".into(),
            Some(Field::Density),
            None,
            None,
        ),
        (
            "Requested infill pattern".into(),
            Some(Field::Pattern),
            None,
            None,
        ),
        (
            "Requested top shell layers".into(),
            Some(Field::Top),
            None,
            None,
        ),
        (
            "Requested bottom shell layers".into(),
            Some(Field::Bottom),
            None,
            None,
        ),
        ("Print preset".into(), Some(Field::Preset), None, None),
        (
            "Print preset behavior".into(),
            None,
            None,
            Some("Presets copy requests; saved parts keep their own values.".into()),
        ),
        (
            "Print preset name".into(),
            Some(Field::PresetName),
            None,
            None,
        ),
        (
            "Save print preset".into(),
            None,
            Some(Command::SavePreset),
            None,
        ),
        (
            "Delete print preset".into(),
            None,
            Some(Command::DeletePreset),
            None,
        ),
        (
            "Copy requests from part".into(),
            Some(Field::CopyFrom),
            None,
            None,
        ),
        ("Copy part requests".into(), None, Some(Command::Copy), None),
        (
            "Print capability target".into(),
            Some(Field::Target),
            None,
            None,
        ),
    ];
    if state.modifier_scope {
        rows.splice(2..2, modifiers::rows(state));
        rows.retain(|(_, field, command, _)| {
            !matches!(field, Some(Field::CopyFrom)) && !matches!(command, Some(Command::Copy))
        });
    }
    if state.height_scope {
        rows.splice(2..2, heights::rows(state));
        rows.retain(|(_, field, command, _)| {
            !matches!(field, Some(Field::CopyFrom)) && !matches!(command, Some(Command::Copy))
        });
        if state.height_editor.profile {
            rows.retain(|(_, field, command, _)| {
                !matches!(
                    field,
                    Some(
                        Field::Walls
                            | Field::Density
                            | Field::Pattern
                            | Field::Top
                            | Field::Bottom
                            | Field::Preset
                            | Field::PresetName
                    )
                ) && !matches!(command, Some(Command::SavePreset | Command::DeletePreset))
            });
        }
    }
    if let Some(document) = &state.document {
        rows.push((
            "Selected process profile".into(),
            None,
            None,
            Some(
                document
                    .selected_process
                    .as_ref()
                    .map(|p| format!("{} · {:?}", p.name, p.status))
                    .unwrap_or_else(|| "Unspecified; inherited values remain unresolved".into()),
            ),
        ));
    }
    let project_effective = state
        .document
        .as_ref()
        .filter(|_| state.project)
        .map(|document| {
            let (settings, sources) =
                limo_cad_core::resolve_print_settings(document, &PrintSettingsDto::default());
            let target = serde_json::from_value(json!(state.target))
                .unwrap_or(limo_cad_core::PrintIntentTargetDto::Portable);
            let configured = limo_cad_core::configured_print_fields(&settings);
            let unsupported: Vec<_> = limo_cad_core::print_setting_capabilities(
                target,
                limo_cad_core::PrintIntentScopeDto::Project,
            )
            .into_iter()
            .filter(|c| !c.supported && configured.contains(&c.field))
            .map(|c| c.field)
            .collect();
            json!({"settings":settings,"sources":sources,"unsupported":unsupported})
        });
    if let Some(part) = project_effective.as_ref().or_else(|| {
        if state.height_scope {
            if state.height_editor.profile {
                None
            } else {
                state.effective["height_ranges"]
                    .as_array()
                    .and_then(|rs| rs.iter().find(|r| heights::selected(state, &r["range"])))
            }
        } else if state.modifier_scope {
            state.effective["modifiers"].as_array().and_then(|ms| {
                ms.iter()
                    .find(|m| m["modifier"]["id"] == state.modifier_selection)
            })
        } else {
            state.effective["parts"].as_array().and_then(|p| p.first())
        }
    }) {
        if !state.project {
            rows.push((
                "Print source binding".into(),
                None,
                None,
                Some(part["binding"].as_str().unwrap_or("unresolved").into()),
            ));
        }
        for (label, field) in [
            ("Walls", Field::Walls),
            ("Infill (%)", Field::Density),
            ("Pattern", Field::Pattern),
            ("Top shell layers", Field::Top),
            ("Bottom shell layers", Field::Bottom),
        ] {
            let key = field_key(field);
            let value = &part["settings"][key];
            let value = if value.is_null() {
                "Unresolved".into()
            } else {
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string())
            };
            let source = part["sources"][key].as_str().unwrap_or("unspecified");
            let unsupported = part["unsupported"]
                .as_array()
                .is_some_and(|fields| fields.iter().any(|f| f.as_str() == Some(key)));
            rows.push((
                format!(
                    "Saved effective {label}{}",
                    if state.project {
                        " · project defaults"
                    } else if state.height_scope {
                        " ? print-Z range"
                    } else if state.modifier_scope {
                        " · local modifier"
                    } else {
                        ""
                    }
                ),
                None,
                None,
                Some(format!(
                    "{value} · {source}{}",
                    if unsupported {
                        " · unsupported by target"
                    } else {
                        ""
                    }
                )),
            ));
        }
    }
    if state.modifier_scope {
        modifiers::report_rows(state, &mut rows);
    }
    if let Some(warnings) = state.effective["warnings"].as_array() {
        for (i, warning) in warnings.iter().enumerate() {
            rows.push((
                format!("Print intent warning {}", i + 1),
                None,
                None,
                Some(warning.as_str().unwrap_or("").into()),
            ));
        }
    }
    if state.dirty() {
        rows.push((
            "Draft state".into(),
            None,
            None,
            Some("Unapplied requests; effective values describe the saved document".into()),
        ));
    }
    for (key, (_, error)) in &state.errors {
        rows.push((format!("Invalid {key}"), None, None, Some(error.clone())));
    }
    if let Some(error) = &state.error {
        rows.push((
            "Print settings conflict".into(),
            None,
            None,
            Some(error.clone()),
        ));
    }
    if state.document.is_none() {
        rows.push((
            "Loading print settings".into(),
            None,
            None,
            Some("Loading owned document settings on the modeling worker".into()),
        ));
    }
    state.scroll = state.scroll.min(rows.len().saturating_sub(per_page));
    let total = rows.len();
    for (i, (label, field, command, information)) in rows
        .into_iter()
        .skip(state.scroll)
        .take(per_page)
        .enumerate()
    {
        let ry = y + 53. + i as f32 * 51.;
        let displayed_label = field
            .and_then(|field| state.errors.get(field_key(field)))
            .map(|(_, error)| format!("{label} · {error}"))
            .unwrap_or_else(|| label.clone());
        state.widgets.text(
            world,
            camera,
            &format!("print-intent-label-{label}"),
            chrome::rect(x + 12., ry, w - 24., 16.),
            &displayed_label,
            10.,
            51,
        );
        let mut control = InterfaceControl::button("body/print-intent", &label);
        control.disabled = worker::busy(world) || state.document.is_none();
        control.disabled |= modifiers::disabled(state, field, command);
        control.disabled |= heights::disabled(state, field, command);
        if state.height_scope
            && !heights::has_draft(state)
            && matches!(
                field,
                Some(Field::Walls | Field::Density | Field::Pattern | Field::Top | Field::Bottom)
            )
        {
            control.disabled = true;
        }
        if state.modifier_scope
            && state.modifier_draft.is_none()
            && matches!(
                field,
                Some(Field::Walls | Field::Density | Field::Pattern | Field::Top | Field::Bottom)
            )
        {
            control.disabled = true;
        }
        let mut caption = None;
        if let Some(field) = field {
            let value = text(state, field);
            if let Some(options) = choices(world, state, field) {
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
            } else {
                control.owned_keys = vec![KeyChord::plain("Enter"), KeyChord::plain("Escape")];
                control.field = ControlField::Text {
                    value,
                    read_only: false,
                    selection: None,
                };
            }
        } else if let Some(value) = information {
            control.field = ControlField::Text {
                value,
                read_only: true,
                selection: None,
            };
        }
        state.widgets.button(
            world,
            camera,
            &format!("print-intent-row-{label}"),
            control,
            caption.as_deref(),
            NativeCommand::PrintIntent(
                state.generation,
                command.unwrap_or_else(|| field.map(Command::Field).unwrap_or(Command::Info)),
            ),
            chrome::rect(x + 12., ry + 17., w - 24., 28.),
            None,
            51,
        )?;
    }
    for (label, caption, command, offset, w, disabled) in [
        (
            "Previous print settings fields",
            "↑",
            Command::Scroll(-1),
            12.,
            28.,
            state.scroll == 0,
        ),
        (
            "More print settings fields",
            "↓",
            Command::Scroll(1),
            43.,
            28.,
            state.scroll + per_page >= total,
        ),
        (
            "Reset print settings to inheritance",
            "Inherit",
            Command::Inherit,
            82.,
            76.,
            false,
        ),
        (
            "Discard print settings draft",
            "Discard",
            Command::Discard,
            164.,
            76.,
            false,
        ),
        (
            "Apply print settings",
            "Apply",
            Command::Apply,
            246.,
            76.,
            false,
        ),
        (
            "Close print settings",
            "Close",
            Command::Close,
            328.,
            76.,
            false,
        ),
    ] {
        let mut control = InterfaceControl::button("body/print-intent", label);
        control.disabled = disabled
            || heights::disabled(state, None, Some(command))
            || (state.modifier_scope
                && state.modifier_draft.is_none()
                && matches!(command, Command::Apply | Command::Inherit))
            || worker::busy(world)
            || (matches!(command, Command::Apply) && !state.errors.is_empty())
            || (state.document.is_none()
                && !matches!(command, Command::Close | Command::Scroll(_)));
        state.widgets.button(
            world,
            camera,
            &format!("print-intent-{label}"),
            control,
            Some(caption),
            NativeCommand::PrintIntent(state.generation, command),
            chrome::rect(x + offset, y + h - 38., w, 26.),
            None,
            51,
        )?;
    }
    Ok(())
}
