//! Native File controls use the existing exchange DTOs, kernel and file writer.
use super::*;
use crate::session_bridge::parse_engine_envelope;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use limo_cad_core::BodyId;
use limo_cad_export::MeshExportScope;
use limo_cad_solid::{
    BodyFeatureDefinitionDto, HoleDefinitionDto, StepExportRequest, StepOccurrencePlacementDto,
    StepThreadMetadataDto,
};

const MAX_STEP_IMPORT_BYTES: u64 = 96 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Step,
    ThreeMf,
    Stl,
}
impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::ThreeMf => "3mf",
            Self::Stl => "stl",
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Step => "STEP AP242",
            Self::ThreeMf => "3MF (millimetres)",
            Self::Stl => "STL mesh (millimetres)",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ExportIntent {
    pub format: Format,
    pub scope: MeshExportScope,
    pub slicer_target: limo_cad_export::SlicerTarget,
    pub named_view: Option<String>,
    pub print_bed: Option<limo_cad_core::PrintBedDto>,
    pub layout_report: Option<Value>,
    pub allow_layout_issues: bool,
    pub(super) bambu: super::bambu::Settings,
    pub(super) body_ids: Vec<BodyId>,
    pub(super) occurrence_id: Option<u64>,
    pub(super) selected: bool,
}

pub(super) fn capture(
    world: &World,
    services: &NativeServices,
    receipt: &DocumentReceipt,
    format: Format,
    selected: bool,
) -> Result<Arc<ExportIntent>, String> {
    named_views::ensure_exportable(world)?;
    print_intent::ensure_clean(world)?;
    services
        .bridge
        .with_native_document_receipt(&services.engine, &receipt.owner, |revision| {
            check_revision(receipt, revision)?;
            let (document, _, presentation, _) = native_viewport::interface_view(world);
            if document != receipt.owner.document_id {
                return Err("The viewport is still changing documents".into());
            }
            let scene = services.engine.solid_scene_snapshot();
            if !scene.errors.is_empty() {
                return Err("Resolve timeline errors before exporting".into());
            }
            let body_ids: Vec<_> = scene
                .bodies
                .iter()
                .filter(|body| !selected || presentation.selected_body_ids.contains(&body.id.0))
                .map(|body| body.id)
                .collect();
            if body_ids.is_empty() {
                return Err(if selected {
                    "Select a body to export"
                } else {
                    "There are no bodies to export"
                }
                .into());
            }
            let intent = Arc::new(ExportIntent {
                format,
                scope: MeshExportScope::Assembly,
                slicer_target: if format == Format::ThreeMf {
                    body_appearance::preferences::read()?
                } else {
                    Default::default()
                },
                named_view: None,
                print_bed: None,
                layout_report: None,
                allow_layout_issues: false,
                bambu: default(),
                body_ids,
                occurrence_id: (selected && presentation.selected_body_ids.len() == 1)
                    .then_some(presentation.selected_occurrence_id)
                    .flatten(),
                selected,
            });
            Ok(intent)
        })
}

fn fresh_layout_report(engine: &AppState, intent: &ExportIntent) -> Result<Option<Value>, String> {
    Ok(if needs_layout_check(intent) {
        Some(parse_engine_envelope(engine.engine_call(
            "print_layout_check",
            &layout_arguments(intent).to_string(),
        ))?)
    } else {
        None
    })
}

pub(super) fn needs_layout_check(intent: &ExportIntent) -> bool {
    intent.format == Format::ThreeMf
        && intent.scope == MeshExportScope::Assembly
        && (!intent.bambu.enabled
            || intent.bambu.placement
                != limo_cad_export::bambu_project::BambuPlacementMode::Template)
}
pub(super) fn layout_arguments(intent: &ExportIntent) -> Value {
    json!({"name":intent.named_view,"bed":intent.print_bed,"body_ids":intent.body_ids})
}

pub(super) fn layout_has_issues(intent: &ExportIntent) -> bool {
    bambu::target_layout_has_issues(intent)
        || (needs_layout_check(intent)
            && intent
                .layout_report
                .as_ref()
                .is_some_and(|r| r["issues"].as_array().is_some_and(|v| !v.is_empty())))
}

pub(super) fn view_key(intent: &ExportIntent) -> String {
    match intent.named_view.as_deref() {
        None => "current".into(),
        Some("") => "assembled".into(),
        Some(name) => format!("saved:{name}"),
    }
}
pub(super) fn view_choices(
    engine: &AppState,
) -> Result<Vec<limo_cad_interface::ChoiceOption>, String> {
    let views: limo_cad_sketch::NamedViewsDto = serde_json::from_value(parse_engine_envelope(
        engine.engine_call("named_views", ""),
    )?)
    .map_err(|e| e.to_string())?;
    let mut choices = vec![
        limo_cad_interface::ChoiceOption {
            value: "current".into(),
            label: "Current displayed view (including live visibility)".into(),
            disabled: false,
        },
        limo_cad_interface::ChoiceOption {
            value: "assembled".into(),
            label: "Assembled model".into(),
            disabled: false,
        },
    ];
    choices.extend(
        views
            .views
            .into_iter()
            .map(|v| limo_cad_interface::ChoiceOption {
                value: format!("saved:{}", v.name),
                label: format!(
                    "Saved view: {}{}",
                    v.name,
                    if v.print_layout {
                        " · print layout"
                    } else {
                        ""
                    }
                ),
                disabled: false,
            }),
    );
    Ok(choices)
}
pub(super) fn bed_key(intent: &ExportIntent) -> String {
    intent
        .print_bed
        .as_ref()
        .and_then(|bed| {
            named_views::printer_choices()
                .into_iter()
                .find(|(_, _, value)| value == bed)
                .map(|(key, _, _)| key)
        })
        .unwrap_or_else(|| "layout".into())
}
pub(super) fn bed_choices() -> Vec<limo_cad_interface::ChoiceOption> {
    let mut choices = vec![limo_cad_interface::ChoiceOption {
        value: "layout".into(),
        label: "Use view bed (or default Bambu X2D)".into(),
        disabled: false,
    }];
    choices.extend(
        named_views::printer_choices()
            .into_iter()
            .map(|(key, label, _)| limo_cad_interface::ChoiceOption {
                value: key,
                label,
                disabled: false,
            }),
    );
    choices
}

pub(super) fn check_layout_confirmation(intent: &ExportIntent) -> Result<(), String> {
    if needs_layout_check(intent) && intent.layout_report.is_none() {
        return Err("Wait for the print layout check before exporting".into());
    }
    let report = if needs_layout_check(intent) {
        intent.layout_report.as_ref()
    } else {
        None
    };
    check_report_confirmation(
        report,
        bambu::target_layout_has_issues(intent),
        intent.allow_layout_issues,
    )
}

fn check_report_confirmation(
    report: Option<&Value>,
    target_has_issues: bool,
    allow_issues: bool,
) -> Result<(), String> {
    let has_issues = target_has_issues
        || report.is_some_and(|report| {
            report["issues"]
                .as_array()
                .is_some_and(|issues| !issues.is_empty())
        });
    if has_issues && !allow_issues {
        return Err("Review the reported layout issues and explicitly choose Export despite layout issues, or correct the reported CAD layout or saved template".into());
    }
    Ok(())
}

fn check_revision(receipt: &DocumentReceipt, revision: u64) -> Result<(), String> {
    if revision != receipt.revision {
        return Err("The document changed while choosing the exchange file".into());
    }
    Ok(())
}

fn picker(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    receipt: DocumentReceipt,
    kind: PickerKind,
) -> Result<Value, String> {
    if world.resource::<Files>().picker.is_some() {
        return Err("A file chooser is already open".into());
    }
    let active = tabs(world, services, &receipt.owner)?
        .into_iter()
        .find(|tab| tab.active)
        .ok_or("Active tab disappeared")?;
    let exporting = match &kind {
        PickerKind::Export(intent) => Some(intent.clone()),
        _ => None,
    };
    let keep_options = exporting
        .as_ref()
        .is_some_and(|intent| intent.bambu.enabled);
    let (dialog, parent) = parented_dialog(world, rfd::FileDialog::new())?;
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-exchange-picker".into())
        .spawn(move || {
            let _parent = parent;
            let mut dialog = dialog;
            if let Some(parent) = active.path.as_ref().and_then(|p| p.parent()) {
                dialog = dialog.set_directory(parent);
            }
            let path = if let Some(intent) = exporting {
                let suffix = if intent.selected && intent.body_ids.len() == 1 {
                    format!("-Body{}", intent.body_ids[0].0)
                } else {
                    String::new()
                };
                let name = format!(
                    "{}{suffix}.{}",
                    active
                        .name
                        .replace(['<', '>', ':', '"', '/', '\\', '|', '?', '*'], "_"),
                    intent.format.extension()
                );
                dialog
                    .add_filter(intent.format.description(), &[intent.format.extension()])
                    .set_file_name(name)
                    .save_file()
            } else {
                dialog.add_filter("STEP/STP", &["step", "stp"]).pick_file()
            };
            let _ = send.send(path);
            wake.request_redraw();
        })
        .map_err(|e| format!("Cannot open file chooser: {e}"))?;
    world.resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind,
        result: Mutex::new(receive),
    });
    if !keep_options {
        world.resource_mut::<Files>().dialog = None;
    }
    Ok(json!({"awaiting_input":true}))
}

pub(super) fn choose_import(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    receipt: DocumentReceipt,
) -> Result<Value, String> {
    picker(world, handle, services, receipt, PickerKind::ImportStep)
}
pub(super) fn choose_export(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    receipt: DocumentReceipt,
    intent: Arc<ExportIntent>,
) -> Result<Value, String> {
    picker(world, handle, services, receipt, PickerKind::Export(intent))
}

fn check_path(path: &std::path::Path, format: Format) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Choose an absolute file path".into());
    }
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if !extension.eq_ignore_ascii_case(format.extension())
        && !(format == Format::Step && extension.eq_ignore_ascii_case("stp"))
    {
        return Err(format!("Choose a .{} file", format.extension()));
    }
    Ok(())
}

pub(super) fn import(
    world: &mut World,
    receipt: DocumentReceipt,
    path: PathBuf,
) -> Result<Value, String> {
    check_path(&path, Format::Step)?;
    let previous: Vec<_> = native_viewport::interface_geometry(world)
        .scene
        .bodies
        .iter()
        .map(|b| b.id.0)
        .collect();
    worker::enqueue_transaction(
        world,
        "solid_import_step".into(),
        move |services, guard| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &receipt.owner,
                |revision| check_revision(&receipt, revision),
            )?;
            let metadata =
                std::fs::metadata(&path).map_err(|e| format!("Could not read STEP file: {e}"))?;
            if metadata.len() > MAX_STEP_IMPORT_BYTES {
                return Err("STEP import is limited to 96 MiB".into());
            }
            let bytes =
                limo_cad_project_file::read_binary_file(&path).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_STEP_IMPORT_BYTES {
                return Err("STEP import is limited to 96 MiB".into());
            }
            let file_name = path
                .file_name()
                .ok_or("STEP file has no name")?
                .to_string_lossy();
            services.bridge.apply_native_mutation_at(
                &services.engine,
                &receipt.owner,
                receipt.revision,
                "solid_import_step",
                &json!({"file_name":file_name,"data_base64":STANDARD.encode(bytes)}),
                || guard.validate(),
            )
        },
        move |world, services, result| {
            let result = result?;
            let owner = result.context.clone();
            let mut output = finish_mutation(
                &services.engine,
                &services.bridge,
                world,
                "solid_import_step",
                result,
            );
            if output["render_error"].is_null() {
                let selection =
                    services
                        .bridge
                        .with_native_document_owner(&services.engine, &owner, || {
                            let imported = native_viewport::interface_geometry(world)
                                .scene
                                .bodies
                                .iter()
                                .find(|b| !previous.contains(&b.id.0))
                                .map(|b| b.id.0);
                            let (_, _, mut view, _) =
                                native_viewport::interface_view_snapshot(world);
                            view.selected_body_ids = imported.into_iter().collect();
                            view.selected_occurrence_id = None;
                            view.selected_face_ids.clear();
                            view.selected_edge_ids.clear();
                            native_viewport::apply_interface_view(
                                world,
                                &owner.document_id,
                                None,
                                Some(view),
                            )
                        });
                if let Err(error) = selection {
                    output["presentation_error"] = json!(error);
                }
            }
            Ok(output)
        },
    )
}

fn step_request(
    engine: &AppState,
    intent: &ExportIntent,
    expected_model_json: String,
) -> Result<StepExportRequest, String> {
    let active: std::collections::HashSet<_> = engine.with_document(|document| {
        let features = document.features();
        features
            .features
            .iter()
            .take(features.rollback_index)
            .filter(|f| !f.suppressed)
            .map(|f| f.id)
            .collect()
    });
    let holes: Vec<HoleDefinitionDto> = serde_json::from_value(parse_engine_envelope(
        engine.engine_call("hole_definitions", ""),
    )?)
    .map_err(|e| e.to_string())?;
    let features: Vec<BodyFeatureDefinitionDto> = serde_json::from_value(parse_engine_envelope(
        engine.engine_call("body_feature_definitions", ""),
    )?)
    .map_err(|e| e.to_string())?;
    let mut thread_metadata = Vec::new();
    for hole in holes
        .into_iter()
        .filter(|h| active.contains(&h.feature_id) && intent.body_ids.contains(&h.body_id))
    {
        if let Some(thread) = hole.thread {
            thread_metadata.push(StepThreadMetadataDto {
                body_id: hole.body_id,
                feature_id: hole.feature_id,
                feature_name: hole.name,
                position_count: hole.positions.len().max(1) as u32,
                external: false,
                predrill_diameter: hole.diameter,
                thread,
            });
        }
    }
    for feature in features {
        if let BodyFeatureDefinitionDto::ExternalThread {
            feature_id,
            name,
            body_id,
            cylinder,
            thread,
            ..
        } = feature
        {
            if active.contains(&feature_id) && intent.body_ids.contains(&body_id) {
                thread_metadata.push(StepThreadMetadataDto {
                    body_id,
                    feature_id,
                    feature_name: name,
                    position_count: 1,
                    external: true,
                    predrill_diameter: cylinder.radius * 2.,
                    thread,
                });
            }
        }
    }
    let assembly: limo_cad_sketch::AssemblyDocumentDto = serde_json::from_value(
        parse_engine_envelope(engine.engine_call("assembly_document", ""))?,
    )
    .map_err(|e| e.to_string())?;
    let solution: limo_cad_sketch::AssemblySolutionDto = serde_json::from_value(
        parse_engine_envelope(engine.engine_call("assembly_solution", ""))?,
    )
    .map_err(|e| e.to_string())?;
    if !solution.solved {
        return Err("Resolve assembly errors before exporting STEP".into());
    }
    let occurrences = solution
        .instance_body_poses
        .iter()
        .filter(|p| {
            p.visible
                && intent.body_ids.contains(&p.body_id)
                && intent
                    .occurrence_id
                    .is_none_or(|id| p.occurrence_id.0 == id)
        })
        .map(|p| StepOccurrencePlacementDto {
            occurrence_id: p.occurrence_id.0,
            component_id: p.component_id.0,
            body_id: p.body_id,
            name: assembly
                .component_structure
                .occurrences
                .iter()
                .find(|o| o.id == p.occurrence_id)
                .map(|o| o.name.clone())
                .unwrap_or_else(|| format!("Occurrence {}", p.occurrence_id.0)),
            translation: p.translation,
            rotation: p.rotation,
        })
        .collect();
    Ok(StepExportRequest {
        body_ids: intent.body_ids.clone(),
        expected_model_json: Some(expected_model_json),
        thread_metadata,
        occurrences,
    })
}

pub(super) fn export(
    world: &mut World,
    receipt: DocumentReceipt,
    intent: Arc<ExportIntent>,
    path: PathBuf,
    overwrite: bool,
) -> Result<Value, String> {
    check_path(&path, intent.format)?;
    named_views::ensure_exportable(world)?;
    print_intent::ensure_clean(world)?;
    super::bambu::check_review(&intent)?;
    if intent.layout_report.is_some() {
        check_layout_confirmation(&intent)?;
    }
    let written_receipt = receipt.clone();
    let written_generation = intent.bambu.generation;
    let written_bambu = intent.bambu.enabled;
    worker::enqueue_document_io(
        world,
        format!("export_{}", intent.format.extension()),
        move |services, guard| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &receipt.owner,
                |revision| {
                    check_revision(&receipt, revision)?;
                    guard.validate()?;
                    if intent.bambu.enabled { super::bambu::check_output(&intent, &path)?; }
                    let layout_report = fresh_layout_report(&services.engine, &intent)?;
                    check_report_confirmation(layout_report.as_ref(), super::bambu::target_layout_has_issues(&intent), intent.allow_layout_issues)?;
                    let model = parse_engine_envelope(
                        services.engine.engine_call("project_export_model", ""),
                    )?
                    .as_str()
                    .ok_or("Project export was not text")?
                    .to_owned();
                    let mut project_report = None;
                    let bytes = if intent.bambu.enabled {
                        let (bytes, report) = super::bambu::export_bytes(&services.engine, &intent, &model)?;
                        project_report = Some(report);
                        bytes
                    } else if intent.format == Format::Step {
                        services.engine.export_step(
                            &serde_json::to_string(&step_request(
                                &services.engine,
                                &intent,
                                model,
                            )?)
                            .map_err(|e| e.to_string())?,
                        )?
                    } else {
                        let request = limo_cad_export::MeshExportRequest {
                            expected_model_json: Some(model),
                            body_ids: intent.body_ids.clone(),
                            scope: intent.scope,
                            slicer_target: intent.slicer_target,
                            named_view: (intent.scope == MeshExportScope::Assembly)
                                .then(|| intent.named_view.clone())
                                .flatten(),
                            print_bed: (intent.scope == MeshExportScope::Assembly)
                                .then(|| intent.print_bed.clone())
                                .flatten(),
                            include_appearance: intent.format == Format::ThreeMf,
                            ..default()
                        };
                        let payload = serde_json::to_string(&request).map_err(|e| e.to_string())?;
                        if intent.format == Format::ThreeMf {
                            services.engine.export_3mf(&payload)?
                        } else {
                            services.engine.export_stl(&payload)?
                        }
                    };
                    if overwrite {
                        limo_cad_project_file::write_binary_file_atomic(&path, &bytes)
                    } else {
                        limo_cad_project_file::write_binary_file_new(&path, &bytes)
                    }
                    .map_err(|e| e.to_string())?;
                    let written_template=if intent.bambu.enabled {
                        Some(json!({"encoded":STANDARD.encode(&bytes),"summary":limo_cad_export::bambu_project::inspect_bambu_template(&bytes).map_err(|e|e.to_string())?}))
                    } else { None };
                    Ok(NativeMutationResult {
                        context: receipt.owner.clone(),
                        engine_revision: revision,
                        value: json!({"exported":true,"path":path,"bytes":bytes.len(),"export_mode":if intent.bambu.enabled {"bambu_project"}else{"portable_model"},"report":project_report,"requires_reslicing":intent.bambu.enabled,"written_template":written_template}),
                    })
                },
            )
        },
        move |world, services, result| {
            let mut value = result?.value;
            if written_bambu {
                super::bambu::after_write(
                    world,
                    services,
                    &written_receipt,
                    written_generation,
                    &mut value,
                )?;
            } else {
                value.as_object_mut().unwrap().remove("written_template");
            }
            Ok(value)
        },
    )
}

#[cfg(test)]
mod tests;
