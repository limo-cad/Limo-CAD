use super::*;

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut State,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let w = if state.details { 480. } else { 256. };
    let x = (width - 156. - w).max(284.);
    let y = 132.;
    let h = (height - y - 108.).clamp(210., 428.);
    let per_page = (((h - 102.) / 51.).floor() as usize).clamp(1, 6);
    let mut rows: Vec<(Option<Field>, String, Option<String>)> = [
        (None, "3MF slicer target"),
        (Some(Field::Brand), "Material brand"),
        (Some(Field::Preset), "Material preset"),
        (Some(Field::FilamentType), "Material family"),
        (Some(Field::Color), "Body color (hex)"),
        (Some(Field::ColorName), "Color name"),
        (Some(Field::MaterialName), "Material name"),
    ]
    .into_iter()
    .map(|(field, label)| (field, label.into(), None))
    .collect();
    rows.insert(
        0,
        (
            None,
            "Part print settings".into(),
            Some("Open Print Settings".into()),
        ),
    );
    if state.details {
        rows = property_rows(state.draft.as_ref().unwrap());
    }
    let total = rows.len();
    state.scroll = state.scroll.min(total.saturating_sub(per_page));
    let theme = crate::native_viewport::ui::theme(world);
    workbench::card(
        (&mut state.widgets, world, camera),
        "appearance-card",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        4.,
        45,
    );
    state.widgets.text(
        world,
        camera,
        "appearance-title",
        rect(x + 12., y + 8., w - 24., 18.),
        "Body appearance",
        13.,
        47,
    );
    state.widgets.text(
        world,
        camera,
        "appearance-body",
        rect(x + 12., y + 28., w - 112., 18.),
        &state.name,
        11.,
        47,
    );
    let mut details = InterfaceControl::button("body/appearance", "Material properties");
    details.disabled = worker::busy(world);
    state.widgets.button(
        world,
        camera,
        "material-properties",
        details,
        Some(if state.details {
            "Hide details"
        } else {
            "Properties"
        }),
        NativeCommand::BodyAppearance(state.generation, Command::Details),
        rect(x + w - 96., y + 27., 84., 21.),
        None,
        47,
    )?;
    let draft = state.draft.as_ref().unwrap();
    for (row, (field, label, information)) in rows
        .into_iter()
        .skip(state.scroll)
        .take(per_page)
        .enumerate()
    {
        let field_y = y + 53. + row as f32 * 51.;
        state.widgets.text(
            world,
            camera,
            &format!("appearance-label-{field:?}-{label}"),
            rect(x + 12., field_y, w - 24., 16.),
            &label,
            10.,
            47,
        );
        let mut control = InterfaceControl::button("body/appearance", &label);
        control.disabled = worker::busy(world);
        let options = if information.is_some() {
            None
        } else {
            field.map_or_else(|| Some(slicer_choices()), |field| choices(draft, field))
        };
        let value = information.clone().unwrap_or_else(|| {
            field.map_or_else(
                || state.slicer_target.as_str().into(),
                |field| {
                    state
                        .errors
                        .get(&field)
                        .map_or_else(|| draft.text(field), |error| error.0.clone())
                },
            )
        });
        let mut caption = options
            .as_ref()
            .and_then(|options| options.iter().find(|o| o.value == value))
            .map(|o| o.label.clone());
        if let Some(options) = options {
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
        } else if label == "Part print settings" {
            caption = Some(value);
        } else {
            control.owned_keys = vec![KeyChord::plain("Enter"), KeyChord::plain("Escape")];
            control.field = ControlField::Text {
                value,
                read_only: information.is_some(),
                selection: None,
            };
        }
        state.widgets.button(
            world,
            camera,
            &if information.is_some() {
                format!("appearance-property-{label}")
            } else {
                format!("appearance-field-{field:?}")
            },
            control,
            caption.as_deref(),
            NativeCommand::BodyAppearance(
                state.generation,
                if label == "Part print settings" {
                    Command::PrintSettings
                } else if information.is_some() {
                    Command::Info
                } else {
                    field.map_or(Command::SlicerTarget, Command::Field)
                },
            ),
            rect(x + 12., field_y + 17., w - 24., 28.),
            None,
            47,
        )?;
    }
    for (label, caption, command, offset, disabled) in [
        (
            "Previous appearance fields",
            "↑",
            Command::Scroll(-(per_page as i32)),
            12.,
            state.scroll == 0,
        ),
        (
            "More appearance fields",
            "↓",
            Command::Scroll(per_page as i32),
            43.,
            state.scroll + per_page >= total,
        ),
        ("Reset appearance", "Reset", Command::Reset, 84., false),
        ("Apply appearance", "Apply", Command::Apply, 167., false),
    ] {
        let mut control = InterfaceControl::button("body/appearance", label);
        control.disabled = disabled || worker::busy(world);
        state.widgets.button(
            world,
            camera,
            &format!("appearance-{label}"),
            control,
            Some(caption),
            NativeCommand::BodyAppearance(state.generation, command),
            rect(
                x + offset,
                y + h - 36.,
                if offset < 80. { 27. } else { 76. },
                26.,
            ),
            None,
            47,
        )?;
    }
    if let Some(error) = state
        .errors
        .values()
        .next()
        .map(|error| &error.1)
        .or(state.preference_error.as_ref())
    {
        state.widgets.text(
            world,
            camera,
            "appearance-error",
            rect(x + 12., y + h - 59., w - 24., 20.),
            error,
            10.,
            47,
        );
        world
            .entity_mut(state.widgets.entity("appearance-error").unwrap())
            .insert(TextColor(Color::srgb(0.95, 0.35, 0.3)));
    }
    Ok(())
}

pub(super) fn property_rows(draft: &Draft) -> Vec<(Option<Field>, String, Option<String>)> {
    let row = |label: String, value: String| (None, label, Some(value));
    let Some(material) = &draft.value.material else {
        let mut rows = vec![
            row(
                "Material properties".into(),
                "Custom or legacy material: no resolved property snapshot.".into(),
            ),
            row(
                "Assigned filament diameter".into(),
                format!("{} mm", draft.value.diameter_mm),
            ),
        ];
        if let Some(density) = draft.value.density_g_cm3 {
            rows.push(row(
                "Assigned density · unsourced metadata".into(),
                format!("{density} g/cm^3"),
            ));
        }
        return rows;
    };
    let mut rows = vec![
        row("Material category".into(), material.kind.clone()),
        row(
            "Material catalog identity".into(),
            material.catalog_id.clone(),
        ),
    ];
    for (i, warning) in material.warnings.iter().enumerate() {
        rows.push(row(
            format!("Material data note {}", i + 1),
            warning.clone(),
        ));
    }
    rows.push(row(
        "Available print profiles".into(),
        if material.print_profiles.is_empty() {
            "No sourced print profile".into()
        } else {
            material
                .print_profiles
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
                .join("; ")
        },
    ));
    if material.kind == "plastic" {
        rows.push(row(
            "Assigned filament diameter".into(),
            format!("{} mm", draft.value.diameter_mm),
        ));
        if !material.properties.iter().any(|p| p.name == "Density") {
            if let Some(density) = draft.value.density_g_cm3 {
                rows.push(row(
                    "Assigned density · unsourced metadata".into(),
                    format!("{density} g/cm^3"),
                ));
            }
        }
    }
    let mut properties: Vec<_> = material.properties.iter().collect();
    let priority = |name: &str| {
        [
            "Density",
            "YoungsModulus",
            "YieldStrength",
            "UltimateTensileStrength",
            "PoissonRatio",
        ]
        .iter()
        .position(|p| *p == name)
        .unwrap_or(5)
    };
    properties.sort_by_key(|p| priority(&p.name));
    for property in properties {
        let source = material
            .sources
            .iter()
            .position(|s| s.id == property.source_id)
            .map(|i| i + 1)
            .unwrap_or(0);
        rows.push(row(
            format!(
                "{} · {} [source {}]",
                property.name, property.context, source
            ),
            format!("{} {}", property.value, property.unit),
        ));
    }
    for (index, source) in material.sources.iter().enumerate() {
        rows.push(row(
            format!("Material source: {} · {}", index + 1, source.path),
            format!(
                "{} @ {} | SHA256 {} | {} | {} | {}",
                source.repository,
                source.revision,
                source.sha256,
                source.license,
                source.author,
                source.reference
            ),
        ));
    }
    rows
}
