use super::*;
use limo_cad_interface::{Field as ControlField, KeyChord};

pub(super) type Row = (String, Option<Field>, Option<Command>, Option<String>);
pub(super) fn information(rows: &mut Vec<Row>, label: impl Into<String>, value: impl Into<String>) {
    let label = label.into();
    let characters: Vec<_> = value.into().chars().collect();
    for (index, chunk) in characters.chunks(88).enumerate() {
        rows.push((
            if index == 0 {
                label.clone()
            } else {
                format!("{label} continued {}", index + 1)
            },
            None,
            None,
            Some(chunk.iter().collect()),
        ));
    }
}

pub(in super::super) fn mode(
    world: &mut World,
    camera: Entity,
    widgets: &mut super::super::super::chrome::Widgets,
    engine: &AppState,
    intent: &io::ExportIntent,
    token: u64,
    (x, y, w): (f32, f32, f32),
) -> Result<(), String> {
    field(
        (world, camera, widgets),
        engine,
        intent,
        token,
        Field::Mode,
        "3MF file mode",
        (x, y, w),
    )?;
    Ok(())
}

fn field(
    (world, camera, widgets): (
        &mut World,
        Entity,
        &mut super::super::super::chrome::Widgets,
    ),
    engine: &AppState,
    intent: &io::ExportIntent,
    token: u64,
    field: Field,
    label: &str,
    (x, y, w): (f32, f32, f32),
) -> Result<(), String> {
    let mut control = InterfaceControl::button("file-dialog", label);
    control.modal_scope = Some("file-dialog".into());
    control.disabled = worker::busy(world) || world.resource::<Files>().picker.is_some();
    let value = field_text(intent, field);
    let mut caption = None;
    if matches!(
        field,
        Field::TemplatePath
            | Field::HandoffName
            | Field::OutputPath
            | Field::VerifierPath
            | Field::VerifierTimeout
    ) {
        control.field = ControlField::Text {
            value,
            read_only: false,
            selection: None,
        };
    } else {
        let options = choices(intent, field, engine)?;
        caption = options
            .iter()
            .find(|o| o.value == value)
            .map(|o| o.label.clone());
        control.role = "combobox".into();
        control.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
            .map(KeyChord::plain)
            .into();
        control.field = ControlField::Choice { value, options };
        control.disabled |=
            field == Field::View && intent.bambu.placement == BambuPlacementMode::Template;
    }
    widgets.button(
        world,
        camera,
        &format!("bambu-field-{field:?}"),
        control,
        caption.as_deref(),
        NativeCommand::File(FileCommand::Bambu(
            token,
            intent.bambu.generation,
            Command::Field(field),
        )),
        super::super::super::chrome::rect(x, y, w, 29.),
        None,
        73,
    )?;
    Ok(())
}

pub(in super::super) fn paint(
    (world, camera, widgets): (
        &mut World,
        Entity,
        &mut super::super::super::chrome::Widgets,
    ),
    engine: &AppState,
    intent: &io::ExportIntent,
    token: u64,
    (x, y, w, h): (f32, f32, f32, f32),
    error: Option<&str>,
) -> Result<(), String> {
    let s = &intent.bambu;
    let mut rows: Vec<Row> = vec![
        (
            "Saved Bambu template path".into(),
            Some(Field::TemplatePath),
            None,
            None,
        ),
        (
            "Choose saved Bambu template".into(),
            None,
            Some(Command::Browse),
            None,
        ),
        (
            "Inspect saved Bambu template".into(),
            None,
            Some(Command::Inspect),
            None,
        ),
    ];
    let mut info = |label: &str, value: String| information(&mut rows, label, value);
    if let Some(template) = &s.template {
        let t = &template.summary;
        info(
            "Template printer and process",
            format!(
                "{} / {} / {}",
                t.printer_settings_id, t.printer_model, t.process_settings_id
            ),
        );
        info(
            "Template process requests",
            format!(
                "walls {}; infill {}% {}; shells top {}, bottom {}",
                t.process_defaults
                    .wall_count
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unspecified".into()),
                t.process_defaults
                    .infill_density_percent
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unspecified".into()),
                t.process_defaults
                    .infill_pattern
                    .map(|v| serde_json::to_value(v)
                        .unwrap()
                        .as_str()
                        .unwrap_or("unspecified")
                        .to_owned())
                    .unwrap_or_else(|| "unspecified".into()),
                t.process_defaults
                    .top_shell_layers
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unspecified".into()),
                t.process_defaults
                    .bottom_shell_layers
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unspecified".into())
            ),
        );
        info(
            "Template plates and nozzles",
            format!(
                "{} plates; nozzle mm {:?}; {} normal volume instances",
                t.plate_count,
                t.nozzle_diameter_mm,
                targets(s).len()
            ),
        );
        info(
            "Template filament slots",
            format!(
                "types {:?}; colors {:?}; profiles {:?}",
                t.filament_types, t.filament_colors, t.filament_settings_ids
            ),
        );
        info(
            "Template support slots",
            format!(
                "support {:?}; interface {:?}; logical filament map {}; nozzle map {}",
                t.support_filament,
                t.support_interface_filament,
                t.filament_map,
                t.filament_nozzle_map
            ),
        );
        info("Template source hash", t.template_sha256.clone());
        for (key, value) in &t.native_process_settings {
            info(
                &format!("Template native {key}"),
                format!("{value} · read-only process metadata"),
            );
        }
        for warning in &t.process_capability_warnings {
            info("Template process capability", warning.clone());
        }
    }
    info(
        "CAD manufacturing identity",
        s.document
            .as_ref()
            .and_then(|d| d.source_document_id.clone())
            .unwrap_or_else(|| {
                "Unassigned; apply process defaults or author Print Settings first".into()
            }),
    );
    rows.extend([
        (
            "Apply template process defaults".into(),
            None,
            Some(Command::ApplyProfile),
            None,
        ),
        (
            "Bambu placement source".into(),
            Some(Field::Placement),
            None,
            None,
        ),
        ("Bambu CAD view".into(), Some(Field::View), None, None),
        (
            "Saved Bambu target handoff".into(),
            Some(Field::Handoff),
            None,
            None,
        ),
        (
            "Bambu source occurrence".into(),
            Some(Field::Source),
            None,
            None,
        ),
        (
            "Bambu target volume instance".into(),
            Some(Field::Target),
            None,
            None,
        ),
        (
            "Bind selected source and target".into(),
            None,
            Some(Command::Bind),
            None,
        ),
        (
            "Bambu binding to remove".into(),
            Some(Field::Binding),
            None,
            None,
        ),
        (
            "Remove selected Bambu binding".into(),
            None,
            Some(Command::Unbind),
            None,
        ),
        (
            "Keep template material and color".into(),
            None,
            Some(Command::AllowAppearance),
            None,
        ),
        (
            "Accept reviewed native setting changes".into(),
            None,
            Some(Command::AcceptNativeChanges),
            None,
        ),
        (
            "Preview Bambu project".into(),
            None,
            Some(Command::Preview),
            None,
        ),
        (
            "Bambu handoff name".into(),
            Some(Field::HandoffName),
            None,
            None,
        ),
        (
            "Save written Bambu handoff".into(),
            None,
            Some(Command::SaveHandoff),
            None,
        ),
        (
            "Remove saved Bambu handoff".into(),
            None,
            Some(Command::RemoveHandoff),
            None,
        ),
    ]);
    rows.push(("Bambu source binding coverage".into(),None,None,Some(format!("{} visible CAD occurrences; {} explicit bindings. Every intended repeat must be bound.",s.sources.len(),s.bindings.len()))));
    if s.written.is_some() {
        rows.push(("Written Bambu handoff ready".into(),None,None,Some("Current template is the exact written output. Save its handoff explicitly; preview before another write.".into())));
    }
    verification::rows(world, intent, &mut rows);
    if let Some((_, report)) = &s.reviewed {
        rows.push((
            "Bambu preview qualification".into(),
            None,
            None,
            Some(format!(
                "Metadata parsed: {}; slicer imported: {}; toolpaths generated: {}",
                report.metadata_readback_verified,
                report.installed_slicer_imported,
                report.toolpaths_generated
            )),
        ));
        rows.push((
            "Bambu reslicing required".into(),
            None,
            None,
            Some(format!(
                "{} derived entries invalidated. Open and reslice the written project.",
                report.invalidated_entries.len()
            )),
        ));
        for (i, part) in report.parts.iter().enumerate() {
            let label = format!("Bambu effective part {}", i + 1);
            rows.push((
                label.clone(),
                None,
                None,
                Some(format!(
                    "body {} occurrence {} -> plate {:?}, filament {} {:?} {:?}",
                    part.binding.body_id.0,
                    part.binding.occurrence_id,
                    part.plate_index,
                    part.filament_index,
                    part.filament_type,
                    part.filament_color
                )),
            ));
            rows.push((
                format!("{label} stable target"),
                None,
                None,
                Some(format!(
                    "object {} instance {} volume {}; UUID {:?}",
                    part.binding.object_id,
                    part.binding.instance_id,
                    part.binding.part_id,
                    part.target_uuid
                )),
            ));
            rows.push((
                format!("{label} geometry SHA256"),
                None,
                None,
                Some(part.geometry_sha256.clone()),
            ));
            rows.push((
                format!("{label} effective settings"),
                None,
                None,
                Some(format!(
                    "walls {}; infill {} {}; top {}; bottom {}",
                    part.effective_settings
                        .get("wall_loops")
                        .map(String::as_str)
                        .unwrap_or("unspecified"),
                    part.effective_settings
                        .get("sparse_infill_density")
                        .map(String::as_str)
                        .unwrap_or("unspecified"),
                    part.effective_settings
                        .get("sparse_infill_pattern")
                        .map(String::as_str)
                        .unwrap_or("unspecified"),
                    part.effective_settings
                        .get("top_shell_layers")
                        .map(String::as_str)
                        .unwrap_or("unspecified"),
                    part.effective_settings
                        .get("bottom_shell_layers")
                        .map(String::as_str)
                        .unwrap_or("unspecified")
                )),
            ));
            for (key, origin) in &part.effective_sources {
                rows.push((
                    format!("{label} {key} origin"),
                    None,
                    None,
                    Some(format!(
                        "{} = {} · {:?}; override {}; inherited {}",
                        key,
                        part.effective_settings
                            .get(key)
                            .map(String::as_str)
                            .unwrap_or("unspecified"),
                        origin,
                        part.written_overrides
                            .get(key)
                            .map(String::as_str)
                            .unwrap_or("absent"),
                        part.inherited_settings
                            .get(key)
                            .map(String::as_str)
                            .unwrap_or("unavailable")
                    )),
                ));
            }
            for (key, value) in &part.native_effective_settings {
                if part.effective_settings.contains_key(key) {
                    continue;
                }
                information(
                    &mut rows,
                    format!("{label} native {key}"),
                    format!(
                        "{value} · {:?}; read-only metadata, toolpath realization unverified",
                        part.native_effective_sources.get(key)
                    ),
                );
            }
            for (axis, values) in ["X basis", "Y basis", "Z basis", "translation mm"]
                .into_iter()
                .zip(part.world_transform.chunks(3))
            {
                rows.push((
                    format!("{label} {axis}"),
                    None,
                    None,
                    Some(format!("{:?}", values)),
                ));
            }
        }
        for group in &report.z_preflight {
            let label = format!(
                "Plate {} object {} instance {}",
                group.plate_index, group.object_id, group.instance_id
            );
            information(
                &mut rows,
                format!("{label} print Z bounds"),
                format!(
                    "{:.3} to {:.3} mm; {} normal source volumes",
                    group.world_bounds.min_mm[2],
                    group.world_bounds.max_mm[2],
                    group.source_bindings.len()
                ),
            );
            for binding in &group.source_bindings {
                information(
                    &mut rows,
                    format!("{label} source"),
                    format!(
                        "body {} occurrence {} volume {}",
                        binding.body_id.0, binding.occurrence_id, binding.part_id
                    ),
                );
            }
            for issue in &group.issues {
                information(&mut rows, format!("{label} {}", issue.code), &issue.message);
            }
            if let Some(translation) = group.proposed_translation_mm {
                let target = match group.correction_target {
                    BambuZCorrectionTarget::SavedTemplate => "saved slicer template",
                    BambuZCorrectionTarget::CadPrintLayout => "CAD print layout",
                };
                information(
                    &mut rows,
                    format!("{label} proposed correction"),
                    format!(
                        "Review support intent or translate the whole group by Z {:.3} mm in the {target}, then preview again. Proposal only; XY clearance and supports remain unqualified.",
                        translation[2]
                    ),
                );
            }
        }
        for (i, warning) in report.warnings.iter().enumerate() {
            information(
                &mut rows,
                format!("Bambu preview warning {}", i + 1),
                warning,
            );
        }
    }
    if let Some(report) = &intent.layout_report {
        if io::needs_layout_check(intent) {
            rows.push((
                "Bambu CAD layout check".into(),
                None,
                None,
                Some(format!(
                    "{} instances; {} groups; {} issues",
                    report["printable_instances"],
                    report["printable_groups"],
                    report["issues"].as_array().map_or(0, Vec::len)
                )),
            ));
        }
    }
    rows.extend([
        (
            "Bambu output project path".into(),
            Some(Field::OutputPath),
            None,
            None,
        ),
        (
            "Write reviewed Bambu project".into(),
            None,
            Some(Command::Write),
            None,
        ),
    ]);
    if let Some(error) = error {
        rows.push(("Bambu project issue".into(), None, None, Some(error.into())));
    }
    let per_page = (((h - 150.) / 51.).floor() as usize).max(1);
    let start = s.scroll.min(rows.len().saturating_sub(per_page));
    let total = rows.len();
    for (i, (label, field_id, command, information)) in
        rows.into_iter().skip(start).take(per_page).enumerate()
    {
        let ry = y + 91. + i as f32 * 51.;
        widgets.text(
            world,
            camera,
            &format!("bambu-label-{label}"),
            super::super::super::chrome::rect(x + 16., ry, w - 32., 16.),
            &label,
            10.,
            73,
        );
        if let Some(field_id) = field_id {
            field(
                (world, camera, widgets),
                engine,
                intent,
                token,
                field_id,
                &label,
                (x + 16., ry + 17., w - 32.),
            )?;
            continue;
        }
        let mut control = InterfaceControl::button("file-dialog", &label);
        control.modal_scope = Some("file-dialog".into());
        control.disabled = worker::busy(world) || world.resource::<Files>().picker.is_some();
        if let Some(value) = information {
            control.field = ControlField::Text {
                value,
                read_only: true,
                selection: None,
            };
        }
        match command {
            Some(Command::AllowAppearance) => control.selected = Some(s.allow_appearance),
            Some(Command::AcceptNativeChanges) => control.selected = Some(s.accept_native_changes),
            Some(Command::Write) => control.disabled |= check_review(intent).is_err(),
            Some(Command::SaveHandoff) => control.disabled |= s.written.is_none(),
            Some(Command::ApplyProfile) => control.disabled |= s.template.is_none(),
            Some(Command::Bind | Command::Unbind) => control.disabled |= s.reference.is_some(),
            Some(Command::VerifyStart) => {
                control.disabled |= verification::can_start(world, intent).is_err()
            }
            Some(Command::VerifyPoll | Command::VerifyCancel) => {
                control.disabled |= !verification::has_job(world)
            }
            _ => {}
        }
        widgets.button(
            world,
            camera,
            &format!("bambu-row-{label}"),
            control,
            None,
            NativeCommand::File(FileCommand::Bambu(
                token,
                s.generation,
                command.unwrap_or(Command::Info),
            )),
            super::super::super::chrome::rect(x + 16., ry + 17., w - 32., 29.),
            None,
            73,
        )?;
    }
    for (label, caption, command, dx, disabled) in [
        (
            "Previous Bambu project fields",
            "Previous",
            Command::Scroll(-1),
            16.,
            start == 0,
        ),
        (
            "More Bambu project fields",
            "More",
            Command::Scroll(1),
            112.,
            start + per_page >= total,
        ),
    ] {
        let mut control = InterfaceControl::button("file-dialog", label);
        control.modal_scope = Some("file-dialog".into());
        control.disabled = disabled || worker::busy(world);
        widgets.button(
            world,
            camera,
            &format!("bambu-{label}"),
            control,
            Some(caption),
            NativeCommand::File(FileCommand::Bambu(token, s.generation, command)),
            super::super::super::chrome::rect(x + dx, y + h - 47., 90., 30.),
            None,
            73,
        )?;
    }
    if io::layout_has_issues(intent) {
        let mut control = InterfaceControl::button("file-dialog", "Export despite layout issues");
        control.modal_scope = Some("file-dialog".into());
        control.selected = Some(intent.allow_layout_issues);
        control.disabled = worker::busy(world);
        widgets.button(
            world,
            camera,
            "bambu-layout-override",
            control,
            Some("Allow reported layout issues"),
            NativeCommand::File(FileCommand::ExportAllowIssues(token)),
            super::super::super::chrome::rect(x + 210., y + h - 47., 190., 30.),
            None,
            73,
        )?;
    }
    Ok(())
}
