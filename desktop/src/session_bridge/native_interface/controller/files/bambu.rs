//! Owned saved-project handoff over the shared print-intent and Bambu adapters.
use super::*;
use crate::session_bridge::parse_engine_envelope;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use limo_cad_core::{
    BambuPartBinding, BambuRefreshReference, PrintIntentDocumentDto, PrintTargetHandoffDto,
    ProcessProfileSnapshotDto, ProcessProfileSourceDto, ProcessProfileStatusDto,
};
use limo_cad_export::{bambu_project::*, BambuExportRequest, MeshExportRequest};

mod panel;
pub(super) mod verification;
pub(super) use panel::mode;
pub(super) use panel::paint;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Mode,
    TemplatePath,
    Placement,
    View,
    Source,
    Target,
    Binding,
    HandoffName,
    Handoff,
    OutputPath,
    VerifierPath,
    VerifierTimeout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Field(Field),
    Browse,
    Inspect,
    Bind,
    Unbind,
    Preview,
    ApplyProfile,
    SaveHandoff,
    RemoveHandoff,
    AllowAppearance,
    AcceptNativeChanges,
    Write,
    VerifyStart,
    VerifyPoll,
    VerifyCancel,
    Scroll(i32),
    Info,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Source {
    body_id: limo_cad_core::BodyId,
    occurrence_id: u64,
    label: String,
}
impl Source {
    fn key(&self) -> String {
        format!("body:{}:occurrence:{}", self.body_id.0, self.occurrence_id)
    }
}
#[derive(Clone)]
struct Template {
    path: PathBuf,
    encoded: Arc<String>,
    summary: BambuTemplateSummary,
}
#[derive(Clone, Default)]
pub(super) struct Settings {
    pub enabled: bool,
    pub generation: u64,
    pub scroll: usize,
    path: String,
    template: Option<Template>,
    model: String,
    document: Option<PrintIntentDocumentDto>,
    sources: Vec<Source>,
    source: String,
    target: String,
    binding: String,
    pub placement: BambuPlacementMode,
    bindings: Vec<BambuPartBinding>,
    reference: Option<BambuRefreshReference>,
    allow_appearance: bool,
    accept_native_changes: bool,
    name: String,
    handoff: String,
    output: String,
    reviewed: Option<(Value, BambuProjectReport)>,
    written: Option<BambuProjectReport>,
    verifier_path: String,
    verifier_timeout: String,
}
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BambuSettings")
            .field("enabled", &self.enabled)
            .field("generation", &self.generation)
            .field("scroll", &self.scroll)
            .finish()
    }
}
impl Settings {
    pub(super) fn invalidate(&mut self) {
        self.reviewed = None;
        self.generation = self.generation.saturating_add(1);
    }
    fn template(&self) -> Result<&Template, String> {
        self.template
            .as_ref()
            .ok_or_else(|| "Choose and inspect a complete saved Bambu project first".into())
    }
}

pub(super) fn target_layout_has_issues(intent: &io::ExportIntent) -> bool {
    intent.bambu.enabled
        && intent.bambu.reviewed.as_ref().is_some_and(|(_, report)| {
            report
                .z_preflight
                .iter()
                .any(|group| !group.issues.is_empty())
        })
}

fn option(value: impl Into<String>, label: impl Into<String>) -> limo_cad_interface::ChoiceOption {
    limo_cad_interface::ChoiceOption {
        value: value.into(),
        label: label.into(),
        disabled: false,
    }
}
fn target_key(object: u32, instance: u32, part: u32) -> String {
    format!("object:{object}:instance:{instance}:part:{part}")
}
fn targets(settings: &Settings) -> Vec<(String, String, (u32, u32, u32))> {
    let Some(template) = &settings.template else {
        return vec![];
    };
    template
        .summary
        .objects
        .iter()
        .flat_map(|object| {
            (0..object.instance_count).flat_map(move |instance| {
                object
                    .parts
                    .iter()
                    .filter(|part| part.subtype == "normal_part")
                    .map(move |part| {
                        (
                            target_key(object.object_id, instance, part.part_id),
                            format!(
                                "{} / {} · object {} instance {} part {}",
                                object.name, part.name, object.object_id, instance, part.part_id
                            ),
                            (object.object_id, instance, part.part_id),
                        )
                    })
            })
        })
        .collect()
}
fn choices(
    intent: &io::ExportIntent,
    field: Field,
    engine: &AppState,
) -> Result<Vec<limo_cad_interface::ChoiceOption>, String> {
    let settings = &intent.bambu;
    Ok(match field {
        Field::Mode => vec![
            option("portable_model", "Portable model"),
            option("bambu_project", "Bambu project · saved template"),
        ],
        Field::Placement => vec![
            option("resolved_scene", "Use CAD displayed/saved layout"),
            option("template", "Keep saved template plates and orientation"),
        ],
        Field::View => io::view_choices(engine)?,
        Field::Source => settings
            .sources
            .iter()
            .map(|s| option(s.key(), &s.label))
            .collect(),
        Field::Target => targets(settings)
            .into_iter()
            .map(|(key, label, _)| option(key, label))
            .collect(),
        Field::Binding => settings
            .bindings
            .iter()
            .enumerate()
            .map(|(i, b)| {
                option(
                    i.to_string(),
                    format!(
                        "Body {} occurrence {} → object {} instance {} part {}",
                        b.body_id.0, b.occurrence_id, b.object_id, b.instance_id, b.part_id
                    ),
                )
            })
            .collect(),
        Field::Handoff => std::iter::once(option("", "No saved handoff"))
            .chain(
                settings
                    .document
                    .iter()
                    .flat_map(|d| d.target_handoffs.iter())
                    .map(|h| option(h.name(), h.name())),
            )
            .collect(),
        _ => return Err("Not a choice field".into()),
    })
}
fn field_text(intent: &io::ExportIntent, field: Field) -> String {
    let s = &intent.bambu;
    match field {
        Field::Mode => if s.enabled {
            "bambu_project"
        } else {
            "portable_model"
        }
        .into(),
        Field::TemplatePath => s.path.clone(),
        Field::Placement => if s.placement == BambuPlacementMode::Template {
            "template"
        } else {
            "resolved_scene"
        }
        .into(),
        Field::View => io::view_key(intent),
        Field::Source => s.source.clone(),
        Field::Target => s.target.clone(),
        Field::Binding => s.binding.clone(),
        Field::HandoffName => s.name.clone(),
        Field::Handoff => s.handoff.clone(),
        Field::OutputPath => s.output.clone(),
        Field::VerifierPath => s.verifier_path.clone(),
        Field::VerifierTimeout => {
            if s.verifier_timeout.is_empty() {
                "120".into()
            } else {
                s.verifier_timeout.clone()
            }
        }
    }
}
fn request(intent: &io::ExportIntent) -> Result<BambuExportRequest, String> {
    request_with_template(intent, true)
}
fn request_with_template(
    intent: &io::ExportIntent,
    include_bytes: bool,
) -> Result<BambuExportRequest, String> {
    let s = &intent.bambu;
    let template = s.template()?;
    if intent.scope != limo_cad_export::MeshExportScope::Assembly {
        return Err(
            "Bambu projects use assembly/layout scope; choose Portable model for definitions"
                .into(),
        );
    }
    let source_document_id=s.document.as_ref().and_then(|d|d.source_document_id.clone())
        .ok_or("Use template process defaults or author Print Settings to assign this document's source identity before preview")?;
    if s.model.is_empty() {
        return Err("Inspect the template against the current document first".into());
    }
    Ok(BambuExportRequest {
        export: MeshExportRequest {
            expected_model_json: Some(s.model.clone()),
            body_ids: intent.body_ids.clone(),
            scope: intent.scope,
            slicer_target: limo_cad_export::SlicerTarget::BambuStudio,
            include_appearance: true,
            named_view: intent.named_view.clone(),
            print_bed: intent.print_bed.clone(),
            ..default()
        },
        project: BambuProjectRequest {
            source_document_id,
            // Saved UUID/instance identities resolve numeric IDs against the
            // current native file; old object numbers must not override them.
            bindings: if s.reference.is_some() {
                vec![]
            } else {
                s.bindings.clone()
            },
            placement: s.placement,
            allow_template_appearance: s.allow_appearance,
            refresh_reference: s.reference.clone(),
            accept_native_setting_changes: s.accept_native_changes,
        },
        template_base64: if include_bytes {
            (*template.encoded).clone()
        } else {
            String::new()
        },
    })
}
fn review_key(request: &BambuExportRequest, summary: &BambuTemplateSummary) -> Value {
    json!({"template_sha256":summary.template_sha256,"export":request.export,"project":request.project})
}
pub(super) fn check_review(intent: &io::ExportIntent) -> Result<(), String> {
    if !intent.bambu.enabled {
        return Ok(());
    }
    let request = request_with_template(intent, false)?;
    let (reviewed, _) = intent.bambu.reviewed.as_ref().ok_or(
        "Preview the Bambu project with its current bindings and confirmations before writing",
    )?;
    if reviewed != &review_key(&request, &intent.bambu.template()?.summary) {
        return Err("Bambu project choices changed; preview them again before writing".into());
    }
    Ok(())
}
pub(super) fn check_output(
    intent: &io::ExportIntent,
    path: &std::path::Path,
) -> Result<(), String> {
    if let Some(template) = &intent.bambu.template {
        if path.exists()
            && std::fs::canonicalize(path).map_err(|e| e.to_string())?
                == std::fs::canonicalize(&template.path).map_err(|e| e.to_string())?
        {
            return Err("The input template is read-only. Choose a separate output project".into());
        }
    }
    Ok(())
}
pub(super) fn export_bytes(
    engine: &AppState,
    intent: &io::ExportIntent,
    current_model: &str,
) -> Result<(Vec<u8>, Value), String> {
    check_review(intent)?;
    let request = request(intent)?;
    request.validate(current_model).map_err(|e| e.to_string())?;
    let value = parse_engine_envelope(engine.engine_call(
        "solid_export_bambu_project",
        &serde_json::to_string(&request).map_err(|e| e.to_string())?,
    ))?;
    let bytes = STANDARD
        .decode(
            value["bytes_base64"]
                .as_str()
                .ok_or("Bambu writer returned no bytes")?,
        )
        .map_err(|e| e.to_string())?;
    Ok((bytes, value["report"].clone()))
}

fn context(engine: &AppState, intent: &io::ExportIntent) -> Result<Value, String> {
    let model = parse_engine_envelope(engine.engine_call("project_export_model", ""))?;
    let document = parse_engine_envelope(engine.engine_call("print_intent_get", ""))?;
    let solution: limo_cad_sketch::AssemblySolutionDto =
        serde_json::from_value(parse_engine_envelope(engine.engine_call(
            "named_view_solution",
            &json!({"name":intent.named_view}).to_string(),
        ))?)
        .map_err(|e| e.to_string())?;
    if !solution.solved {
        return Err("Resolve assembly/layout errors before binding a Bambu project".into());
    }
    let scene = engine.solid_scene_snapshot();
    let assembly: limo_cad_sketch::AssemblyDocumentDto = serde_json::from_value(
        parse_engine_envelope(engine.engine_call("assembly_document", ""))?,
    )
    .map_err(|e| e.to_string())?;
    let sources: Vec<_> = solution
        .instance_body_poses
        .into_iter()
        .filter(|p| p.visible && intent.body_ids.contains(&p.body_id))
        .map(|p| {
            let body = scene
                .bodies
                .iter()
                .find(|b| b.id == p.body_id)
                .map(|b| b.name.as_str())
                .unwrap_or("Retained source");
            let occurrence = assembly
                .component_structure
                .occurrences
                .iter()
                .find(|o| o.id == p.occurrence_id)
                .map(|o| o.name.as_str())
                .unwrap_or("Occurrence");
            Source {
                body_id: p.body_id,
                occurrence_id: p.occurrence_id.0,
                label: format!(
                    "{occurrence} / {body} · body {} occurrence {}",
                    p.body_id.0, p.occurrence_id.0
                ),
            }
        })
        .collect();
    Ok(json!({"model":model,"document":document,"sources":sources}))
}
fn accept_context(s: &mut Settings, value: &Value) -> Result<(), String> {
    s.model = value["model"]
        .as_str()
        .ok_or("Missing owned model snapshot")?
        .into();
    s.document =
        Some(serde_json::from_value(value["document"].clone()).map_err(|e| e.to_string())?);
    s.sources = serde_json::from_value(value["sources"].clone()).map_err(|e| e.to_string())?;
    if !s.sources.iter().any(|source| source.key() == s.source) {
        s.source = s.sources.first().map(Source::key).unwrap_or_default();
    }
    Ok(())
}

pub(super) fn after_write(
    world: &mut World,
    services: &NativeServices,
    receipt: &DocumentReceipt,
    generation: u64,
    value: &mut Value,
) -> Result<(), String> {
    let snapshot = value
        .as_object_mut()
        .ok_or("Bambu write omitted its receipt")?
        .remove("written_template")
        .ok_or("Bambu write omitted its owned template snapshot")?;
    services.bridge.with_native_document_receipt(&services.engine,&receipt.owner,|revision|{
        if revision!=receipt.revision{return Err("The document changed after writing the Bambu project; its file was written but handoff selection was not changed".into())}
        let Some(dialog)=world.resource_mut::<Files>().into_inner().dialog.as_mut() else{return Ok(())};
        if dialog.receipt.owner!=receipt.owner||dialog.receipt.revision!=receipt.revision{return Ok(())}
        let DialogKind::Export(intent)=&mut dialog.kind else{return Ok(())};
        let intent=Arc::make_mut(intent);
        let s=&mut intent.bambu;
        if !s.enabled||s.generation!=generation{return Ok(())}
        let report:BambuProjectReport=serde_json::from_value(value["report"].clone()).map_err(|e|e.to_string())?;
        let path:PathBuf=serde_json::from_value(value["path"].clone()).map_err(|e|e.to_string())?;
        s.template=Some(Template{path:path.clone(),encoded:Arc::new(snapshot["encoded"].as_str().ok_or("Missing written template bytes")?.into()),summary:serde_json::from_value(snapshot["summary"].clone()).map_err(|e|e.to_string())?});
        s.path=path.to_string_lossy().into();
        s.reference=Some(report.refresh_reference.clone());
        s.written=Some(report);
        s.handoff.clear();
        s.output.clear();
        s.scroll = 0;
        s.invalidate();
        Ok(())
    })
}
fn owned(
    world: &World,
    services: &NativeServices,
    owner: &DocumentContext,
    token: u64,
    generation: u64,
) -> Result<Dialog, String> {
    let dialog = owned_dialog(world, services, owner, token)?;
    let DialogKind::Export(intent) = &dialog.kind else {
        return Err("Not a 3MF export dialog".into());
    };
    if intent.format != io::Format::ThreeMf || intent.bambu.generation != generation {
        return Err("The Bambu project controls changed".into());
    }
    Ok(dialog)
}
fn current_settings<'a>(
    world: &'a mut World,
    token: u64,
    generation: u64,
    owner: &DocumentContext,
) -> Result<&'a mut io::ExportIntent, String> {
    let dialog = world
        .resource_mut::<Files>()
        .into_inner()
        .dialog
        .as_mut()
        .ok_or("Export dialog closed")?;
    if dialog.token != token || &dialog.receipt.owner != owner {
        return Err("Export dialog was replaced".into());
    }
    let DialogKind::Export(intent) = &mut dialog.kind else {
        return Err("Export dialog changed".into());
    };
    if intent.bambu.generation != generation {
        return Err("Bambu choices changed while the operation was running".into());
    }
    Ok(Arc::make_mut(intent))
}

pub(super) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    (services, owner): (&NativeServices, &DocumentContext),
    token: u64,
    generation: u64,
    command: Command,
    input: &ControlInput,
) -> Result<Value, String> {
    require_idle_model(world)?;
    if matches!(command, Command::VerifyPoll | Command::VerifyCancel) {
        if !super::super::super::is_activation(input) {
            return Err("Activate the local verification command".into());
        }
        return verification::reduce(world, services, owner, token, command);
    }
    let receipt = owned(world, services, owner, token, generation)?.receipt;
    if !matches!(command, Command::Field(_)) && !super::super::super::is_activation(input) {
        return Err("Activate the Bambu project command".into());
    }
    if command == Command::Info {
        return Ok(json!({"read_only":true}));
    }
    let field_value = if let Command::Field(field) = command {
        let dialog = world
            .resource::<Files>()
            .dialog
            .as_ref()
            .ok_or("Export dialog closed")?;
        let DialogKind::Export(intent) = &dialog.kind else {
            unreachable!()
        };
        Some(
            if matches!(
                field,
                Field::TemplatePath
                    | Field::HandoffName
                    | Field::OutputPath
                    | Field::VerifierPath
                    | Field::VerifierTimeout
            ) {
                let ControlInput::SetValue(value) = input else {
                    return Ok(json!({"focused":true}));
                };
                value.clone()
            } else {
                workbench::cam::choose(
                    &choices(intent, field, &services.engine)?,
                    &field_text(intent, field),
                    input,
                )?
            },
        )
    } else {
        None
    };
    let dialog = world
        .resource_mut::<Files>()
        .into_inner()
        .dialog
        .as_mut()
        .ok_or("Export dialog closed")?;
    let DialogKind::Export(intent) = &mut dialog.kind else {
        unreachable!()
    };
    let intent = Arc::make_mut(intent);
    if let Command::Field(field) = command {
        let value = field_value.expect("field command value");
        dialog.error = None;
        match field {
            Field::Mode => {
                if value == "bambu_project"
                    && intent.scope != limo_cad_export::MeshExportScope::Assembly
                {
                    return Err(
                        "Choose Assembly/layout scope before selecting Bambu project mode".into(),
                    );
                }
                intent.bambu.enabled = value == "bambu_project";
                intent.bambu.scroll = 0;
            }
            Field::TemplatePath => {
                intent.bambu.path = value;
                intent.bambu.template = None;
                intent.bambu.written = None;
                intent.bambu.bindings.clear();
                intent.bambu.reference = None;
            }
            Field::Placement => {
                intent.bambu.placement = if value == "template" {
                    BambuPlacementMode::Template
                } else {
                    BambuPlacementMode::ResolvedScene
                };
                if intent.bambu.placement == BambuPlacementMode::Template {
                    intent.named_view = None;
                }
                intent.layout_report = None;
                intent.allow_layout_issues = false;
            }
            Field::View => {
                intent.named_view = if value == "current" {
                    None
                } else if value == "assembled" {
                    Some(String::new())
                } else {
                    Some(
                        value
                            .strip_prefix("saved:")
                            .ok_or("Choose a saved CAD view")?
                            .into(),
                    )
                };
                intent.layout_report = None;
                intent.allow_layout_issues = false;
            }
            Field::Source => intent.bambu.source = value,
            Field::Target => intent.bambu.target = value,
            Field::Binding => intent.bambu.binding = value,
            Field::HandoffName => intent.bambu.name = value,
            Field::OutputPath => intent.bambu.output = value,
            Field::VerifierPath | Field::VerifierTimeout => {
                if value.len() > 4096 {
                    return Err("Local slicer option exceeds 4096 bytes".into());
                }
                if field == Field::VerifierPath {
                    intent.bambu.verifier_path = value;
                } else {
                    intent.bambu.verifier_timeout = value;
                }
            }
            Field::Handoff => {
                let reference = if value.is_empty() {
                    None
                } else {
                    let handoff = intent
                        .bambu
                        .document
                        .as_ref()
                        .and_then(|d| d.target_handoffs.iter().find(|h| h.name() == value))
                        .ok_or("Saved handoff was removed")?;
                    let PrintTargetHandoffDto::BambuStudio { reference, .. } = handoff;
                    Some(reference.clone())
                };
                intent.bambu.written = None;
                intent.bambu.handoff = value.clone();
                intent.bambu.reference = None;
                intent.bambu.bindings.clear();
                if let Some(reference) = reference {
                    intent.bambu.bindings =
                        reference.parts.iter().map(|p| p.binding.clone()).collect();
                    intent.bambu.reference = Some(reference);
                    intent.bambu.name = value;
                }
            }
        }
        if matches!(
            field,
            Field::Source
                | Field::Target
                | Field::Binding
                | Field::HandoffName
                | Field::OutputPath
                | Field::VerifierPath
                | Field::VerifierTimeout
        ) {
            intent.bambu.generation = intent.bambu.generation.saturating_add(1);
        } else {
            intent.bambu.invalidate();
        }
        if matches!(field, Field::View | Field::Placement) {
            return refresh_context(world, services, owner, token);
        }
        return Ok(json!({"changed":true}));
    }
    if !matches!(command, Command::Info | Command::Scroll(_)) {
        dialog.error = None;
    }
    match command {
        Command::Info => unreachable!(),
        Command::Scroll(direction) => {
            intent.bambu.scroll = if direction < 0 {
                intent.bambu.scroll.saturating_sub(1)
            } else {
                intent.bambu.scroll.saturating_add(1)
            }
        }
        Command::AllowAppearance => {
            intent.bambu.allow_appearance = !intent.bambu.allow_appearance;
            intent.bambu.invalidate();
        }
        Command::AcceptNativeChanges => {
            intent.bambu.accept_native_changes = !intent.bambu.accept_native_changes;
            intent.bambu.invalidate();
        }
        Command::Bind => {
            if intent.bambu.reference.is_some() {
                return Err(
                    "Choose No saved handoff before authoring replacement numeric bindings".into(),
                );
            }
            let source = intent
                .bambu
                .sources
                .iter()
                .find(|s| s.key() == intent.bambu.source)
                .ok_or("Choose a CAD occurrence")?;
            let (_, _, (object_id, instance_id, part_id)) = targets(&intent.bambu)
                .into_iter()
                .find(|(k, _, _)| k == &intent.bambu.target)
                .ok_or("Choose a normal template volume instance")?;
            if intent.bambu.bindings.iter().any(|b| {
                (b.body_id == source.body_id && b.occurrence_id == source.occurrence_id)
                    || (b.object_id == object_id
                        && b.instance_id == instance_id
                        && b.part_id == part_id)
            }) {
                return Err("This source occurrence or target volume instance is already bound. Remove its old binding before replacing it".into());
            }
            intent.bambu.bindings.push(BambuPartBinding {
                body_id: source.body_id,
                occurrence_id: source.occurrence_id,
                object_id,
                instance_id,
                part_id,
            });
            intent.bambu.invalidate();
        }
        Command::Unbind => {
            if intent.bambu.reference.is_some() {
                return Err(
                    "Choose No saved handoff before authoring replacement numeric bindings".into(),
                );
            }
            let index = intent
                .bambu
                .binding
                .parse::<usize>()
                .map_err(|_| "Choose a binding to remove")?;
            if index >= intent.bambu.bindings.len() {
                return Err("The selected binding was removed".into());
            }
            intent.bambu.bindings.remove(index);
            intent.bambu.binding.clear();
            intent.bambu.invalidate();
        }
        Command::Inspect => {
            let path = PathBuf::from(&intent.bambu.path);
            return inspect(world, services, owner, token, path);
        }
        Command::Browse => return browse(world, handle, receipt, token, generation),
        Command::Preview => return preview(world, services, owner, token),
        Command::VerifyStart | Command::VerifyPoll | Command::VerifyCancel => {
            return verification::reduce(world, services, owner, token, command);
        }
        Command::ApplyProfile | Command::SaveHandoff | Command::RemoveHandoff => {
            return metadata(world, services, owner, token, command);
        }
        Command::Write => {
            check_review(intent)?;
            io::check_layout_confirmation(intent)?;
            let path = PathBuf::from(&intent.bambu.output);
            check_output(intent, &path)?;
            let DialogKind::Export(intent) = &dialog.kind else {
                unreachable!()
            };
            let intent = Arc::clone(intent);
            return io::export(world, receipt, intent, path, false);
        }
        Command::Field(_) => unreachable!(),
    }
    intent.bambu.generation = intent.bambu.generation.saturating_add(1);
    Ok(json!({"changed":true}))
}

fn browse(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    receipt: DocumentReceipt,
    token: u64,
    generation: u64,
) -> Result<Value, String> {
    if world.resource::<Files>().picker.is_some() {
        return Err("A file chooser is already open".into());
    }
    let (dialog, parent) = parented_dialog(
        world,
        rfd::FileDialog::new().add_filter("Saved Bambu 3MF project", &["3mf"]),
    )?;
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-bambu-template-picker".into())
        .spawn(move || {
            let _parent = parent;
            let path = dialog.pick_file();
            let _ = send.send(path);
            wake.request_redraw();
        })
        .map_err(|e| e.to_string())?;
    world.resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind: PickerKind::BambuTemplate { token, generation },
        result: Mutex::new(receive),
    });
    Ok(json!({"awaiting_input":true}))
}
pub(super) fn selected_path(
    world: &mut World,
    services: &NativeServices,
    receipt: DocumentReceipt,
    token: u64,
    generation: u64,
    path: PathBuf,
) -> Result<Value, String> {
    owned(world, services, &receipt.owner, token, generation)?;
    inspect(world, services, &receipt.owner, token, path)
}
fn inspect(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    token: u64,
    path: PathBuf,
) -> Result<Value, String> {
    let dialog = owned_dialog(world, services, owner, token)?;
    let DialogKind::Export(intent) = dialog.kind else {
        return Err("Not an export dialog".into());
    };
    let generation = intent.bambu.generation;
    let receipt = dialog.receipt;
    let callback_owner = owner.clone();
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("3mf"))
    {
        return Err("Choose an absolute .3mf template path".into());
    }
    worker::enqueue_document_io(
        world,
        "inspect_bambu_template".into(),
        move |services, guard| {
            services.bridge.with_native_document_receipt(&services.engine,&receipt.owner,|revision|{
            if revision!=receipt.revision{return Err("The document changed before template inspection".into())}guard.validate()?;
            if std::fs::metadata(&path).map_err(|e|e.to_string())?.len()>128*1024*1024{return Err("Bambu template input is limited to128MiB".into())}
            let bytes=limo_cad_project_file::read_binary_file(&path).map_err(|e|e.to_string())?;
            if bytes.len()>128*1024*1024{return Err("Bambu template input is limited to128MiB".into())}
            let encoded=STANDARD.encode(bytes);let summary=parse_engine_envelope(services.engine.engine_call("bambu_template_inspect",&json!({"template_base64":encoded}).to_string()))?;
            Ok(NativeMutationResult{context:receipt.owner.clone(),engine_revision:revision,value:json!({"path":path,"encoded":encoded,"summary":summary,"context":context(&services.engine,&intent)?})})
        })
        },
        move |world, services, result| {
            let result = result?;
            services.bridge.with_native_document_receipt(&services.engine,&callback_owner,|revision|{
            if revision!=result.engine_revision{return Err("The document changed during template inspection".into())}
            let intent=current_settings(world,token,generation,&callback_owner)?;
            let summary:BambuTemplateSummary=serde_json::from_value(result.value["summary"].clone()).map_err(|e|e.to_string())?;
            let path:PathBuf=serde_json::from_value(result.value["path"].clone()).map_err(|e|e.to_string())?;
            let s=&mut intent.bambu;accept_context(s,&result.value["context"])?;s.path=path.to_string_lossy().into();
            s.template=Some(Template{path,encoded:Arc::new(result.value["encoded"].as_str().ok_or("Missing template bytes")?.into()),summary:summary.clone()});
                s.written=None;s.bindings.clear();s.reference=None;s.handoff.clear();s.allow_appearance=false;s.accept_native_changes=false;s.target=targets(s).first().map(|t|t.0.clone()).unwrap_or_default();s.invalidate();
            Ok(json!({"inspected":true,"template":summary,"source_instances":s.sources.len()}))
        })
        },
    )
}
fn refresh_context(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    token: u64,
) -> Result<Value, String> {
    let dialog = owned_dialog(world, services, owner, token)?;
    let DialogKind::Export(intent) = dialog.kind else {
        unreachable!()
    };
    let generation = intent.bambu.generation;
    let receipt = dialog.receipt;
    let callback_owner = owner.clone();
    worker::enqueue_document_io(
        world,
        "refresh_bambu_sources".into(),
        move |services, guard| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &receipt.owner,
                |revision| {
                    if revision != receipt.revision {
                        return Err("The document changed while choosing a Bambu layout".into());
                    }
                    guard.validate()?;
                    Ok(NativeMutationResult {
                        context: receipt.owner.clone(),
                        engine_revision: revision,
                        value: context(&services.engine, &intent)?,
                    })
                },
            )
        },
        move |world, services, result| {
            let result = result?;
            services.bridge.with_native_document_receipt(
                &services.engine,
                &callback_owner,
                |revision| {
                    if revision != result.engine_revision {
                        return Err("The document changed during Bambu source refresh".into());
                    }
                    accept_context(
                        &mut current_settings(world, token, generation, &callback_owner)?.bambu,
                        &result.value,
                    )
                },
            )?;
            queue_layout_check(world, services, &callback_owner, token)
        },
    )
}
fn preview(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    token: u64,
) -> Result<Value, String> {
    let dialog = owned_dialog(world, services, owner, token)?;
    let DialogKind::Export(intent) = dialog.kind else {
        unreachable!()
    };
    let request = request(&intent)?;
    let key = review_key(&request, &intent.bambu.template()?.summary);
    let generation = intent.bambu.generation;
    let callback_owner = owner.clone();
    worker::enqueue_query(
        world,
        dialog.receipt.owner,
        dialog.receipt.revision,
        "bambu_project_preview".into(),
        serde_json::to_value(request).map_err(|e| e.to_string())?,
        move |world, services, result| {
            let result = result?;
            services.bridge.with_native_document_receipt(
                &services.engine,
                &callback_owner,
                |revision| {
                    if revision != result.engine_revision {
                        return Err("The document changed during Bambu preview".into());
                    }
                    let intent = current_settings(world, token, generation, &callback_owner)?;
                    let report: BambuProjectReport =
                        serde_json::from_value(result.value["report"].clone())
                            .map_err(|e| e.to_string())?;
                    intent.bambu.bindings =
                        report.parts.iter().map(|p| p.binding.clone()).collect();
                    intent.bambu.reviewed = Some((key, report.clone()));
                    intent.bambu.generation = intent.bambu.generation.saturating_add(1);
                    Ok(json!({"previewed":true,"report":report,"requires_reslicing":true}))
                },
            )
        },
    )
}
fn metadata(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    token: u64,
    command: Command,
) -> Result<Value, String> {
    let dialog = owned_dialog(world, services, owner, token)?;
    let DialogKind::Export(intent) = dialog.kind else {
        unreachable!()
    };
    let s = &intent.bambu;
    let template = s.template()?;
    let (operation, mut arguments) = match command {
        Command::ApplyProfile => {
            if let Some(warning) = template.summary.process_capability_warnings.first() {
                return Err(format!(
                    "Template inspection is read-only for this process: {warning}"
                ));
            }
            let mut document = s.document.clone().ok_or("Inspect the template first")?;
            document.selected_process = Some(ProcessProfileSnapshotDto {
                profile_id: template.summary.process_settings_id.clone(),
                name: template.summary.process_settings_id.clone(),
                source: Some(ProcessProfileSourceDto::SavedTemplate {
                    sha256: template.summary.template_sha256.clone(),
                    source_label: template
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                }),
                status: ProcessProfileStatusDto::Resolved,
                defaults: template.summary.process_defaults.clone(),
            });
            ("print_intent_set_document", json!({"document":document}))
        }
        Command::SaveHandoff => {
            let name = s.name.trim();
            if name.is_empty() {
                return Err("Name the target handoff before saving".into());
            }
            let reference = s
                .written
                .as_ref()
                .ok_or(
                    "Write the reviewed Bambu project before saving its verified handoff lineage",
                )?
                .refresh_reference
                .clone();
            (
                "print_intent_upsert_handoff",
                json!({"handoff":PrintTargetHandoffDto::BambuStudio{name:name.into(),source_label:template.path.file_name().unwrap_or_default().to_string_lossy().into(),reference}}),
            )
        }
        Command::RemoveHandoff => {
            if s.handoff.is_empty() {
                return Err("Choose a saved target handoff to remove".into());
            }
            ("print_intent_remove_handoff", json!({"name":s.handoff}))
        }
        _ => unreachable!(),
    };
    arguments["expected_model_json"] = json!(s.model);
    let receipt = dialog.receipt;
    let generation = s.generation;
    let callback_owner = owner.clone();
    let context_intent = intent.clone();
    worker::enqueue_transaction(
        world,
        operation.into(),
        move |services, guard| {
            let mut result = services.bridge.apply_native_mutation_at(
                &services.engine,
                &receipt.owner,
                receipt.revision,
                operation,
                &arguments,
                || guard.validate(),
            )?;
            let loaded = services.bridge.with_native_document_receipt(
                &services.engine,
                &result.context,
                |revision| {
                    if revision != result.engine_revision {
                        return Err("Print handoff changed before its UI refresh".into());
                    }
                    context(&services.engine, &context_intent)
                },
            )?;
            result.value = json!({"written":result.value,"context":loaded});
            Ok(result)
        },
        move |world, services, result| {
            let result = result?;
            let new_receipt = DocumentReceipt {
                owner: result.context.clone(),
                revision: result.engine_revision,
            };
            // Closing export options cannot suppress publication of a metadata
            // edit that already committed on the owned modeling worker.
            if let Ok(intent) = current_settings(world, token, generation, &callback_owner) {
                accept_context(&mut intent.bambu, &result.value["context"])?;
                if command == Command::SaveHandoff {
                    intent.bambu.handoff = intent.bambu.name.trim().into();
                    intent.bambu.reference = intent
                        .bambu
                        .written
                        .as_ref()
                        .map(|r| r.refresh_reference.clone());
                }
                if command == Command::RemoveHandoff {
                    intent.bambu.handoff.clear();
                    intent.bambu.reference = None;
                }
                intent.bambu.invalidate();
                world
                    .resource_mut::<Files>()
                    .dialog
                    .as_mut()
                    .unwrap()
                    .receipt = new_receipt;
            }
            Ok(finish_mutation(
                &services.engine,
                &services.bridge,
                world,
                operation,
                result,
            ))
        },
    )
}
