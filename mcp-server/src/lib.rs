#![recursion_limit = "256"]
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use limo_cad_export::MeshExportRequest;
use limo_cad_mcp_mutate::{self, PayloadKind as Payload};
use limo_cad_occt::OcctKernel;
use limo_cad_sketch::{host, SketchManager};
use limo_cad_solid::{CommitKernelRequest, RecomputePlanDto, StepExportRequest};
use serde_json::{json, Map, Value};

mod assembly_tools;
mod broker;
mod build_pair;
mod cam_tools;
#[cfg(test)]
mod component_edit_tests;
mod computer_control;
mod desktop;
mod disclosure;
#[cfg(test)]
mod drawing_command_tests;
mod drawing_tools;
mod inbox;
mod interface;
mod knowledge;
#[cfg(test)]
mod lesson_tests;
mod local_slicer_tools;
mod manufacturing_tools;
mod print_height_tools;
mod print_intent_tools;
mod print_modifier_tools;
mod prompts;
mod script_export;
mod session;
mod stdio;
mod summary;

pub use session::control_owner_error;
pub use stdio::{
    desktop_mcp_presence, prepare_desktop_stdio, run_desktop_stdio, run_stdio,
    shutdown_desktop_stdio, DesktopMcpPresence,
};

use disclosure::{
    auto_focus_for_tool, tags_for_tool, AdvertisementState, DisclosureMode, DisclosureState,
    FocusPack,
};

const LATEST_PROTOCOL: &str = "2025-06-18";

/// The same recipe catalog powers native discovery and MCP.
pub fn script_examples() -> Value {
    limo_cad_recipes::catalog(true)
}

/// No engine construction is needed to validate or hand off an installed lesson.
pub fn recipe_id_from_uri(uri: &str) -> Result<&'static str, String> {
    limo_cad_recipes::from_open_uri(uri).map(|recipe| recipe.id)
}

/// A URL launch reuses a single unambiguous live window when possible. Ordinary
/// launches remain independent processes, including recording/MCP windows.
pub fn open_recipe_in_running_desktop(recipe: &str) -> Result<bool, String> {
    limo_cad_recipes::find(recipe)?;
    desktop::open_recipe(recipe)
}

/// Inspect the same validated JSONC source accepted by `cad_interface/script`.
///
/// `source` is the expanded script (safe to replay inline). `authored_source`
/// is the text the user opened, including unresolved `includes` and comments.
pub fn inspect_script(arguments: Value) -> Result<Value, String> {
    let loaded = interface::load_script(&arguments)?;
    let script = limo_cad_script::Script::parse(&loaded.expanded)?;
    interface::validate_script(&script)?;
    let mut result = script.metadata();
    result["authored_chapters"] = limo_cad_script::authored_chapters(
        &loaded.authored,
        result["step_count"].as_u64().unwrap_or(0) as usize,
    )?;
    result["source"] = Value::String(loaded.expanded);
    result["authored_source"] = Value::String(loaded.authored);
    if let Some(path) = arguments.get("path") {
        result["path"] = path.clone();
    }
    Ok(result)
}

/// Embedded desktop entry point. Modeling still goes through the live native
/// inbox and the shared interface, exactly as it does for an external MCP call.
pub fn run_script(
    source: &str,
    include_base: Option<&str>,
    session_id: Option<&str>,
    mode: &str,
    speed: f64,
) -> Result<Value, String> {
    let mut arguments = json!({"action":"script","source":source,"mode":mode,"speed":speed});
    if let Some(include_base) = include_base {
        arguments["include_base"] = json!(include_base);
    }
    if let Some(session_id) = session_id {
        arguments["session_id"] = json!(session_id);
    }
    CadServer::new()?.call_tool("cad_interface", arguments)
}

/// A small example can render isolated preview frames without attaching to,
/// opening, or mutating any desktop document. There is no separate interpreter.
pub fn preview_script(source: &str) -> Result<Value, String> {
    if source.len() > 2 * 1024 * 1024 {
        return Err("Preview scripts must be no larger than 2 MiB".into());
    }
    let metadata = limo_cad_script::Script::parse(source)?.metadata();
    let steps = metadata["step_count"].as_u64().unwrap_or(0)
        + metadata["check_count"].as_u64().unwrap_or(0);
    if steps > 80 {
        return Err(
            "Preview supports at most 80 steps and checks; run this script in a new design instead"
                .into(),
        );
    }
    run_script(source, None, None, "fast", 1.0)
}

/// A pause may begin and end between receipt observations. Give resumed
/// execution a fresh bounded wait, always against the original submission.
fn await_playback_receipt(
    mut wait: impl FnMut() -> Result<Value, String>,
    mut presentation_status: impl FnMut() -> Result<Value, String>,
) -> Result<Value, String> {
    let mut active_retry_available = true;
    loop {
        let receipt = wait()?;
        if receipt["status"] != "timeout" {
            return Ok(receipt);
        }
        let state = presentation_status()?;
        if state["status"] != "applied" {
            return Ok(receipt);
        }
        if state["presentation"]["stopped"] == true {
            return Err(json!({"code":"playback_stopped","receipt":receipt}).to_string());
        }
        if state["presentation"]["paused"] == true {
            active_retry_available = true;
            continue;
        }
        if active_retry_available {
            active_retry_available = false;
            continue;
        }
        return Ok(receipt);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Execution {
    Direct,
    SolidReplay,
    Control,
}

struct ToolSpec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    engine_method: &'static str,
    payload: Payload,
    execution: Execution,
    input_schema: Value,
    pack: FocusPack,
    spine: bool,
}

impl ToolSpec {
    fn direct(
        name: &'static str,
        title: &'static str,
        description: &'static str,
        engine_method: &'static str,
        payload: Payload,
        input_schema: Value,
    ) -> Self {
        let (pack, spine) = tags_for_tool(name);
        Self {
            name,
            title,
            description,
            engine_method,
            payload,
            execution: Execution::Direct,
            input_schema,
            pack,
            spine,
        }
    }

    fn solid(
        name: &'static str,
        title: &'static str,
        description: &'static str,
        engine_method: &'static str,
        payload: Payload,
        input_schema: Value,
    ) -> Self {
        let (pack, spine) = tags_for_tool(name);
        Self {
            name,
            title,
            description,
            engine_method,
            payload,
            execution: Execution::SolidReplay,
            input_schema,
            pack,
            spine,
        }
    }

    fn control(
        name: &'static str,
        title: &'static str,
        description: &'static str,
        input_schema: Value,
    ) -> Self {
        let (pack, spine) = tags_for_tool(name);
        Self {
            name,
            title,
            description,
            engine_method: "",
            payload: Payload::Empty,
            execution: Execution::Control,
            input_schema,
            pack,
            spine,
        }
    }
}

struct CadServer {
    computer_control: computer_control::ComputerControl,
    verification_owner_id: String,
    manager: SketchManager,
    kernel: OcctKernel,
    disclosure: DisclosureState,
    /// Session id last successfully loaded via read-only `cad_attach` / `cad_refresh`.
    /// MCP never writes this session's files back (no last-writer-wins vs a UI).
    attached_document_id: Option<String>,
    /// Model publication generation captured at the last successful attach/refresh
    /// (`model_generation` / `published_generation` fence, not live engine
    /// `generation`). Compared by `cad_session_status` against live generation.
    attached_generation: Option<u64>,
    /// Last completed snapshot loaded into the read manager. UI-only controls
    /// can acknowledge without changing it; avoid replaying identical geometry.
    loaded_snapshot_json: Option<String>,
    /// A completed UI transition whose snapshot could not be loaded. Model
    /// commands remain blocked until an explicit attach/refresh succeeds.
    failed_attachment: Option<AttachmentFailure>,
    pending_recompute_transaction: Option<u64>,
    /// Forward record of successful mutating `tools/call` entries for `cad_script`.
    tool_trace: Vec<Value>,
    /// Depth of composite tools (`solid_box`) whose parts must not be traced twice.
    composite_depth: u32,
    /// Expanded source from last successful cad_interface script.
    last_script_source: Option<String>,
    /// Successful modeling mutations in this process, traced or live. A live
    /// (attached) mutation never grows `tool_trace`, so staleness of
    /// `last_script_source` is judged against this count instead.
    modeling_mutations: u64,
    /// `modeling_mutations` when `last_script_source` was captured. A later
    /// mutation means that source no longer describes the session.
    last_script_mutations: u64,
    /// Scripts use authoritative live results without rebuilding a second
    /// OCCT model after each mutation. Snapshot reads still refresh on demand.
    script_running: bool,
    /// Interpreter-owned progress transported with both modes' existing inbox
    /// operations, never counted independently by the host or UI.
    script_progress: Option<limo_cad_script::RunProgress>,
    live_snapshot_dirty: bool,
    /// Desktop stdio starts with this process's live document, never an
    /// invisible independent model. After detach, selection must be explicit.
    desktop_binding: Option<DesktopBinding>,
}

struct DesktopBinding {
    process_id: u32,
    initial_selection_pending: bool,
}

struct AttachmentFailure {
    session_id: String,
    error: String,
}

enum SnapshotRefresh {
    DeferDuringScript,
    Immediate,
}

impl CadServer {
    fn new() -> Result<Self, String> {
        Ok(Self {
            computer_control: computer_control::ComputerControl::default(),
            verification_owner_id: limo_cad_export::slicer_verification::new_verification_owner(),
            manager: SketchManager::new(),
            kernel: OcctKernel::new().map_err(|error| error.to_string())?,
            disclosure: DisclosureState::new(),
            attached_document_id: None,
            attached_generation: None,
            loaded_snapshot_json: None,
            failed_attachment: None,
            pending_recompute_transaction: None,
            tool_trace: Vec::new(),
            composite_depth: 0,
            last_script_source: None,
            modeling_mutations: 0,
            last_script_mutations: 0,
            script_running: false,
            script_progress: None,
            live_snapshot_dirty: false,
            desktop_binding: None,
        })
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        self.ensure_desktop_target(name, &arguments)?;
        let live_mutation = self.attached_document_id.is_some() && is_modeling_mutate(name);
        let trace_args = (records_in_script(name) && !live_mutation && self.composite_depth == 0)
            .then(|| arguments.clone());
        let result = self.dispatch_tool(name, arguments);
        if result.is_ok() && changes_model(name) {
            self.modeling_mutations += 1;
            let _ = limo_cad_export::slicer_verification::local_slicer_service()
                .observe_owned_model(&self.verification_owner_id, || {
                    self.manager
                        .export_project_model()
                        .map_err(|error| error.to_string())
                });
        }
        if result.is_ok() {
            if let Some(trace_args) = trace_args {
                self.tool_trace.push(json!({
                    "name": name,
                    "arguments": trace_args,
                }));
            }
        }
        result
    }

    fn ensure_desktop_target(&mut self, name: &str, arguments: &Value) -> Result<(), String> {
        if let Some(failure) = &self.failed_attachment {
            let independent = if name == "cad_interface" {
                arguments["action"].is_null()
                    || arguments["action"].as_str().is_some_and(|action| {
                        session::is_ui_action(action)
                            || matches!(action, "catalog" | "recipes" | "launch")
                    })
                    || (arguments["action"] == "execute"
                        && arguments["operation"].as_str().is_some_and(|operation| {
                            matches!(operation, "cad_session_status" | "cad_await_apply")
                                && arguments["group"] == interface::group_for(operation).unwrap()
                        }))
                    || (arguments["action"] == "execute"
                        && stdio::independent_of_default_document(name, arguments))
            } else {
                matches!(
                    name,
                    "cad_refresh" | "cad_session_status" | "cad_await_apply"
                ) || stdio::independent_of_default_document(name, arguments)
            };
            if !independent {
                return Err(json!({
                    "code":"attachment_unavailable",
                    "session_id":failure.session_id,
                    "snapshot_error":failure.error,
                    "retained_snapshot_session_id":self.attached_document_id,
                    "model_commands_blocked":true,
                    "hint":"Recover the active snapshot with cad_attach or cad_refresh; cad_detach explicitly releases the live target."
                }).to_string());
            }
            return Ok(());
        }
        let Some(binding) = &self.desktop_binding else {
            return Ok(());
        };
        if self.attached_document_id.is_some()
            || stdio::independent_of_default_document(name, arguments)
        {
            return Ok(());
        }
        if !binding.initial_selection_pending {
            return Err("Desktop MCP has no selected document. Use cad_attach or an explicit interface session_id; use --headless for an independent document.".into());
        }
        let session_id = session::desktop_default_session(binding.process_id)?;
        self.attach_read_only_snapshot(&json!({"session_id":session_id}))?;
        Ok(())
    }

    fn interface_session(&self) -> Option<&str> {
        self.failed_attachment
            .as_ref()
            .map(|failure| failure.session_id.as_str())
            .or(self.attached_document_id.as_deref())
    }

    fn report_attachment_failure(&self, result: &mut Value) {
        let failure = self.failed_attachment.as_ref().unwrap();
        result["attached"] = json!(false);
        result["attached_session_id"] = Value::Null;
        result["snapshot_error"] = json!(failure.error);
        result["model_commands_blocked"] = json!(true);
        result["retained_snapshot_session_id"] = json!(self.attached_document_id);
    }

    /// Preserve a completed native receipt even when the read manager cannot
    /// load its active document. Never leave the previous snapshot callable.
    fn follow_interface_attachment(
        &mut self,
        result: &mut Value,
        active: String,
        refresh: SnapshotRefresh,
    ) -> Result<(), String> {
        if self.script_running && self.attached_document_id.as_deref() != Some(active.as_str()) {
            let error =
                "Active document changed during script playback; no later commands were submitted";
            self.failed_attachment = Some(AttachmentFailure {
                session_id: active,
                error: error.into(),
            });
            self.report_attachment_failure(result);
            return Err(format!("{error}: {result}"));
        }
        if self
            .failed_attachment
            .as_ref()
            .is_some_and(|failure| failure.session_id == active)
        {
            self.report_attachment_failure(result);
            return Ok(());
        }
        let attached = if self.attached_document_id.as_deref() != Some(active.as_str()) {
            self.attach_read_only_snapshot(&json!({"session_id":active}))
                .map(|_| ())
        } else if self.script_running && matches!(refresh, SnapshotRefresh::DeferDuringScript) {
            self.live_snapshot_dirty = true;
            Ok(())
        } else {
            self.load_snapshot_model(&active, true).map(|changed| {
                if changed {
                    self.apply_snapshot_focus(&active);
                }
            })
        };
        match attached {
            Ok(()) => {
                self.failed_attachment = None;
                result["attached"] = json!(true);
                result["attached_session_id"] = json!(active);
                result["model_commands_blocked"] = json!(false);
            }
            Err(error) => {
                self.failed_attachment = Some(AttachmentFailure {
                    session_id: active,
                    error,
                });
                self.report_attachment_failure(result);
            }
        }
        Ok(())
    }

    fn dispatch_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let tools = tool_specs();
        let spec = tools
            .iter()
            .find(|spec| spec.name == name)
            .ok_or_else(|| format!("unknown tool: {name}"))?;
        let execution = spec.execution;
        let engine_method = spec.engine_method;
        let payload_kind = spec.payload;
        let pack = spec.pack;
        let spine = spec.spine;
        let live_query = self
            .attached_document_id
            .as_deref()
            .is_some_and(|session_id| {
                limo_cad_mcp_mutate::is_live_engine_query(engine_method)
                    && (engine_method != "assembly_document"
                        || session::heartbeat_meta(session_id)["interface_version"] == 1)
            });

        if self.live_snapshot_dirty
            && execution != Execution::Control
            && !is_modeling_mutate(name)
            && !live_query
        {
            self.refresh_read_only_snapshot()?;
        }

        if self.attached_document_id.is_some() && is_modeling_mutate(name) {
            return self.execute_interface(
                &json!({"operation":name,"group":interface::group_for(name),"arguments":arguments}),
            );
        }
        if self.attached_document_id.is_some() && !is_read_safe_while_attached(name) {
            return Err(session_lock_error(
                "session_read_only",
                self.attached_document_id.as_deref(),
            ));
        }

        if execution == Execution::Control {
            return self.call_control(name, arguments);
        }

        if self.disclosure.advertisement_state(pack, spine) == AdvertisementState::HiddenButCallable
        {
            self.disclosure.re_promote(pack);
        }

        let payload = limo_cad_mcp_mutate::encode_payload(payload_kind, &arguments)?;

        let mut value = if let Some(session_id) =
            self.attached_document_id.as_deref().filter(|_| live_query)
        {
            session::request_engine_query(session_id, engine_method, &payload)?
        } else if execution == Execution::Direct {
            if name == "drawing_export" {
                let request: limo_cad_occt::drawing_export::DrawingExportRequest =
                    serde_json::from_value(arguments).map_err(|e| e.to_string())?;
                let scene = self.manager.solid_scene_ref();
                let content = limo_cad_occt::drawing_export::export_sheet(
                    self.manager.drawing_document_ref(),
                    scene,
                    self.manager.assembly_document_ref(),
                    &request,
                    |r| {
                        limo_cad_occt::project_drawing(
                            &self.kernel,
                            scene,
                            self.manager.assembly_document_ref(),
                            r,
                        )
                        .map_err(|e| e.to_string())
                    },
                )?;
                json!({"format":request.format,"encoding":"utf8","content":content,"sheet_id":request.sheet_id})
            } else if name == "solid_section_review" {
                let request: limo_cad_occt::section_review::SectionReviewRequest =
                    serde_json::from_value(arguments).map_err(|e| e.to_string())?;
                let review = limo_cad_occt::section_review::inspect(
                    &self.kernel,
                    self.manager.solid_scene_ref(),
                    &request,
                )
                .map_err(|error| error.to_string())?;
                serde_json::to_value(review).map_err(|e| e.to_string())?
            } else if name == "drawing_projection" {
                let request: limo_cad_occt::DrawingProjectionRequest =
                    serde_json::from_value(arguments).map_err(|e| e.to_string())?;
                let scene = self.manager.solid_scene_ref();
                if !scene.errors.is_empty() {
                    return Err("Resolve timeline errors before generating a drawing view.".into());
                }
                let projection = limo_cad_occt::project_drawing(
                    &self.kernel,
                    scene,
                    self.manager.assembly_document_ref(),
                    &request,
                )
                .map_err(|e| e.to_string())?;
                serde_json::to_value(projection).map_err(|e| e.to_string())?
            } else if name == "solid_export_step" {
                let request: StepExportRequest = if arguments.is_null() {
                    StepExportRequest::default()
                } else {
                    serde_json::from_value(arguments)
                        .map_err(|error| format!("invalid STEP export request: {error}"))?
                };
                if request.expected_model_json.is_some() {
                    limo_cad_solid::check_export_model_snapshot(
                        request.expected_model_json.as_deref(),
                        &self
                            .manager
                            .export_project_model()
                            .map_err(|e| e.to_string())?,
                    )?;
                }
                let bytes = self
                    .kernel
                    .export_step(&request)
                    .map_err(|error| error.to_string())?;
                json!({
                    "format": "step",
                    "encoding": "base64",
                    "bytes_base64": BASE64.encode(bytes),
                })
            } else if name == "bambu_template_inspect" {
                manufacturing_tools::inspect(arguments)?
            } else if name == "bambu_local_verification_start" {
                self.start_local_verification(arguments)?
            } else if name == "bambu_local_verification_poll"
                || name == "bambu_local_verification_cancel"
            {
                self.local_verification_status(
                    arguments,
                    name == "bambu_local_verification_cancel",
                )?
            } else if name == "bambu_project_preview" || name == "solid_export_bambu_project" {
                self.export_bambu_project(arguments, name == "bambu_project_preview")?
            } else if name == "solid_export_stl" || name == "solid_export_3mf" {
                self.export_mesh(name, arguments)?
            } else if name == "assembly_evaluate_motion_study" {
                let request = serde_json::from_value(arguments)
                    .map_err(|e| format!("motion evaluation request: {e}"))?;
                serde_json::to_value(limo_cad_occt::evaluate_motion_study(
                    &self.manager,
                    &self.kernel,
                    &request,
                )?)
                .map_err(|e| e.to_string())?
            } else if name == "assembly_swept_collision_check" {
                let request = serde_json::from_value(arguments)
                    .map_err(|e| format!("swept collision request: {e}"))?;
                serde_json::to_value(limo_cad_occt::exact_swept_collision_check(
                    &self.manager,
                    &self.kernel,
                    &request,
                )?)
                .map_err(|e| e.to_string())?
            } else if name == "assembly_interference_check" {
                let request = serde_json::from_value(arguments)
                    .map_err(|e| format!("interference request: {e}"))?;
                let solution = self.manager.assembly_solution();
                if !solution.solved {
                    return Err("Cannot inspect interference in an unsolved assembly".into());
                }
                serde_json::to_value(limo_cad_occt::exact_interference_report(
                    &self.kernel,
                    self.manager.solid_scene_ref(),
                    &solution.instance_body_poses,
                    &request,
                )?)
                .map_err(|e| e.to_string())?
            } else if name == "solid_tessellate" {
                self.tessellate_tool(arguments)?
            } else if name == "solid_export_preflight" {
                self.export_preflight_tool(arguments)?
            } else if name == "demo_export_pip_3mf" {
                self.demo_pip_3mf_tool(arguments)?
            } else if name == "printer_catalog" {
                serde_json::to_value(limo_cad_core::embedded_printer_catalog())
                    .map_err(|e| e.to_string())?
            } else if name == "material_catalog" {
                serde_json::from_str(&limo_cad_export::catalog_json())
                    .map_err(|error| format!("catalog json: {error}"))?
            } else if name == "body_appearances" {
                serde_json::to_value(self.manager.body_appearances())
                    .map_err(|error| format!("encode appearances: {error}"))?
            } else if name == "set_body_appearance" {
                let appearances = parse_engine_envelope(host::handle(
                    &mut self.manager,
                    engine_method,
                    &payload,
                ))?;
                json!({"body_appearances":appearances})
            } else {
                parse_engine_envelope(host::handle(&mut self.manager, engine_method, &payload))?
            }
        } else {
            let plan_value =
                parse_engine_envelope(host::handle(&mut self.manager, engine_method, &payload))?;
            let plan: RecomputePlanDto = serde_json::from_value(plan_value)
                .map_err(|error| format!("engine returned an invalid recompute plan: {error}"))?;
            let transaction_id = plan.transaction_id;
            self.pending_recompute_transaction = Some(transaction_id);
            let queries = self.manager.history_support_queries();
            let (scene, verified) = match self.kernel.recompute_with_supports(&plan, &queries) {
                Ok(scene) => scene,
                Err(error) => {
                    self.manager.cancel_solid_recompute(transaction_id);
                    self.pending_recompute_transaction = None;
                    return Err(error.to_string());
                }
            };
            let commit = CommitKernelRequest {
                transaction_id,
                scene,
            };
            let committed = serde_json::to_value(
                self.manager
                    .commit_solid_with_verified_supports(commit, &verified)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            self.pending_recompute_transaction = None;
            committed
        };

        if let Some(focus) = auto_focus_for_tool(name) {
            self.disclosure.auto_hint(focus);
        }
        value = annotate_disclosure(value, &self.disclosure, pack, spine);
        Ok(value)
    }

    fn call_control(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let value = match name {
            "solid_box" => self.solid_box(arguments)?,
            "print_calibrate" | "print_crop" | "print_probe" | "print_symbols" => {
                self.print_tool(name, arguments)?
            }
            "cad_get_focus" => self.disclosure.status_json(),
            "cad_set_focus" => {
                let focus_name = arguments
                    .get("focus")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "missing required argument 'focus'".to_string())?;
                let focus = FocusPack::parse(focus_name)
                    .ok_or_else(|| format!("unknown focus '{focus_name}'"))?;
                let explicit = arguments
                    .get("explicit")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.disclosure.set_focus(focus, explicit);
                self.disclosure.status_json()
            }
            "cad_list_focus_areas" => DisclosureState::focus_areas_json(),
            "cad_get_tool_disclosure_mode" => {
                json!({ "mode": self.disclosure.status_json()["mode"] })
            }
            "cad_set_tool_disclosure_mode" => {
                let mode_name = arguments
                    .get("mode")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "missing required argument 'mode'".to_string())?;
                let mode = DisclosureMode::parse(mode_name)
                    .ok_or_else(|| format!("unknown disclosure mode '{mode_name}'"))?;
                self.disclosure.set_mode(mode);
                json!({ "mode": mode.as_str() })
            }
            "cad_list_all_tools" => full_tool_catalog(),
            "cad_help" => cad_help_call(&arguments)?,
            "cad_cancel_recompute" => {
                if let Some(transaction_id) = self.pending_recompute_transaction.take() {
                    self.manager.cancel_solid_recompute(transaction_id);
                    json!({ "cancelled": true, "transaction_id": transaction_id })
                } else {
                    json!({ "cancelled": false, "reason": "no in-flight solid recompute" })
                }
            }
            "cad_list_sessions" => session::sessions_list_json(),
            "cad_computer_control" => {
                let result = self
                    .computer_control
                    .call(&arguments, self.attached_document_id.as_deref())?;
                if matches!(
                    result["status"].as_str(),
                    Some("input_sent" | "input_incomplete")
                ) {
                    self.live_snapshot_dirty = true;
                }
                result
            }
            "cad_route" => broker::call(arguments)?,
            "cad_interface" => {
                if arguments["action"].is_null() || arguments["action"] == "catalog" {
                    json!({"groups":interface::groups(),"operations":full_tool_catalog()})
                } else if arguments["action"] == "recipes" {
                    limo_cad_recipes::catalog(false)
                } else if arguments["action"] == "execute" {
                    self.execute_interface(&arguments)?
                } else if arguments["action"] == "script" {
                    self.execute_script(&arguments)?
                } else if arguments["action"] == "summary" {
                    self.feature_summary(&arguments)?
                } else if arguments["action"] == "check" {
                    self.check_features(&arguments)?
                } else if arguments["action"] == "export_script" {
                    self.export_script(&arguments)?
                } else if arguments["action"] == "open_recipe" {
                    session::request_ui(&arguments, self.interface_session())?
                } else if arguments["action"] == "launch" {
                    let mut launched = desktop::launch(&arguments)?;
                    if launched["status"] == "ready" {
                        let active = launched["session_id"]
                            .as_str()
                            .ok_or("Ready desktop has no session ID")?
                            .to_owned();
                        self.follow_interface_attachment(
                            &mut launched,
                            active,
                            SnapshotRefresh::DeferDuringScript,
                        )?;
                    }
                    launched
                } else {
                    let mut result = session::request_ui(&arguments, self.interface_session())?;
                    if arguments["action"] == "inspect" {
                        build_pair::decorate(&mut result);
                    }
                    if result["status"] == "applied" {
                        if let Some(active) =
                            result["active_session_id"].as_str().map(str::to_owned)
                        {
                            self.follow_interface_attachment(
                                &mut result,
                                active,
                                SnapshotRefresh::DeferDuringScript,
                            )?;
                        }
                    }
                    result
                }
            }
            "cad_attach" => self.attach_read_only_snapshot(&arguments)?,
            "cad_refresh" => self.refresh_read_only_snapshot()?,
            "cad_detach" => {
                let previous = self.attached_document_id.take();
                let previous = self
                    .failed_attachment
                    .take()
                    .map(|failure| failure.session_id)
                    .or(previous);
                self.attached_generation = None;
                self.loaded_snapshot_json = None;
                self.live_snapshot_dirty = false;
                if let Some(binding) = &mut self.desktop_binding {
                    binding.initial_selection_pending = false;
                }
                json!({
                    "detached": true,
                    "session_id": previous,
                    "session_mode": "read_only_snapshot",
                })
            }
            "cad_script" => {
                if self.live_snapshot_dirty {
                    self.refresh_read_only_snapshot()?;
                }
                json!({ "calls": self.tool_trace.clone() })
            }
            "cad_compare_solids" => {
                if let Some(session_id) = self.attached_document_id.as_deref() {
                    let mut receipt = broker::inspect_solid_scene(session_id)?;
                    let scene = receipt
                        .as_object_mut()
                        .and_then(|receipt| receipt.remove("value"))
                        .ok_or_else(|| {
                            json!({"code":"missing_live_solid_scene",
                                "message":"Live solid scene query omitted its scene",
                                "source":receipt})
                            .to_string()
                        })?;
                    let scene: limo_cad_solid::SolidSceneDto = serde_json::from_value(scene)
                        .map_err(|error| {
                            json!({"code":"invalid_live_solid_scene",
                                "message":format!("Live solid scene query returned invalid scene: {error}"),
                                "source":receipt}).to_string()
                        })?;
                    let mut summary = compare_solids_summary(&scene);
                    summary["source"] = receipt;
                    summary
                } else {
                    compare_solids_summary(self.manager.solid_scene_ref())
                }
            }
            "cad_submit" => self.submit_inbox_op(&arguments)?,
            "cad_await_apply" => self.await_inbox_apply(&arguments)?,
            "cad_session_status" => self.session_status()?,
            other => return Err(format!("unknown control tool: {other}")),
        };
        Ok(value)
    }

    fn execute_script(&mut self, arguments: &Value) -> Result<Value, String> {
        if self.script_running {
            return Err("Scripts cannot recursively run another script".into());
        }
        let source = interface::script_source(arguments)?;
        let script = limo_cad_script::Script::parse(&source)?;
        interface::validate_script(&script)?;
        if let Some(session_id) = arguments.get("session_id") {
            let session_id = session_id
                .as_str()
                .ok_or("script session_id must be a string")?;
            self.attach_read_only_snapshot(&json!({"session_id":session_id}))?;
        }
        if arguments.get("mode").is_some_and(|mode| !mode.is_string()) {
            return Err("script mode must be fast or present".into());
        }
        let mode = arguments
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("fast");
        if !matches!(mode, "fast" | "present") {
            return Err("script mode must be fast or present".into());
        }
        if arguments
            .get("validate")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err("script validate must be a boolean".into());
        }
        let detail_full = match arguments.get("detail").and_then(Value::as_str) {
            None | Some("compact") => false,
            Some("full") => true,
            Some(_) => return Err("script detail must be compact or full".into()),
        };
        let presentation = mode == "present";
        if presentation && self.attached_document_id.is_none() {
            return Err(
                "Presentation requires an attached desktop; use fast mode headlessly".into(),
            );
        }
        if self.attached_document_id.is_some() {
            self.refresh_read_only_snapshot()?;
        }
        let active_sketch = self.call_tool("sketch_active", json!({}))?;
        if !active_sketch.is_null() || !self.manager.is_blank_for_script() {
            return Err(
                "Script requires a blank document; create a new file before running it".into(),
            );
        }
        let options = limo_cad_script::RunOptions {
            presentation,
            validate: arguments
                .get("validate")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        };
        script.validate_options(options)?;
        let step_count = script.metadata()["step_count"].clone();
        if self.attached_document_id.is_some() {
            let mut configure = json!({"action":"presentation","command":"configure","mode":mode,
                "step_index":0,"step_count":step_count,"chapter":"","text":""});
            if let Some(speed) = arguments.get("speed") {
                configure["speed"] = speed.clone();
            }

            self.script_running = true;
            let configured = self.call_tool("cad_interface", configure);
            match configured {
                Ok(result) if result["status"] == "applied" => {}
                other => {
                    self.script_running = false;
                    return Err(match other {
                        Ok(result) => format!("Playback configuration failed: {result}"),
                        Err(error) => error,
                    });
                }
            }
        }
        self.script_running = true;
        let mut result = limo_cad_script::run_with_progress(
            &script,
            |name, arguments, progress| {
                self.script_progress = Some(progress);
                let is_note =
                    arguments["action"] == "presentation" && arguments["command"] == "note";
                let result = self.call_tool(name, arguments)?;
                if name == "cad_interface"
                    && result
                        .get("status")
                        .is_some_and(|status| status != "applied")
                {
                    return Err(format!("Interface operation failed: {result}"));
                }
                if is_note {
                    self.wait_for_script_presentation(&result)?;
                }
                Ok(result)
            },
            options,
        );
        self.script_running = false;
        self.script_progress = None;
        if self.live_snapshot_dirty {
            if let Err(error) = self.refresh_read_only_snapshot() {
                if result.is_ok() {
                    result = Err(error);
                }
            }
        }
        if self.attached_document_id.is_some() {
            let presentation_result = match &result {
                Ok(report) => json!({"action":"presentation","command":"finish",
                    "step_index":report["steps_completed"],"step_count":step_count}),
                Err(_) => {
                    json!({"action":"presentation","command":"stop","text":"Playback stopped. The partial design is preserved; see the script error for details."})
                }
            };
            let acknowledged = self.call_tool("cad_interface", presentation_result);
            if result.is_ok() {
                match acknowledged {
                    Ok(value) if value["status"] == "applied" => {}
                    Ok(value) => {
                        result = Err(format!("Playback completion was not acknowledged: {value}"))
                    }
                    Err(error) => result = Err(format!("Playback completion failed: {error}")),
                }
            }
        }
        if result.is_ok() {
            self.last_script_mutations = self.modeling_mutations;
            self.last_script_source = Some(source);
        }

        result.map(|mut report| {
            let scene = self.manager.solid_scene_ref();
            let definitions = self.manager.hole_definitions();
            let built = summary::summarize(scene, &definitions);
            report["summary"] = built.json(detail_full);
            report["warnings"] = Value::Array(summary::warnings(
                &built,
                &definitions,
                &script.unused_bindings(),
            ));
            report
        })
    }

    /// The built model in feature terms: bodies with bounding boxes and every
    /// hole with its position, diameter, depth, face and thread, plus the
    /// warnings the script report would carry.
    fn feature_summary(&mut self, arguments: &Value) -> Result<Value, String> {
        let full = match arguments.get("detail").and_then(Value::as_str) {
            None | Some("full") => true,
            Some("compact") => false,
            Some(_) => return Err("summary detail must be compact or full".into()),
        };
        if self.attached_document_id.is_some() {
            self.refresh_read_only_snapshot()?;
        }
        let scene = self.manager.solid_scene_ref();
        let definitions = self.manager.hole_definitions();
        let built = summary::summarize(scene, &definitions);
        let mut value = built.json(full);
        value["warnings"] = Value::Array(summary::warnings(&built, &definitions, &[]));
        Ok(value)
    }

    /// Compare an expected feature table with the built model: bounding box
    /// extents and holes matched by position within a tolerance.
    fn check_features(&mut self, arguments: &Value) -> Result<Value, String> {
        let expected = arguments
            .get("expected")
            .ok_or("check requires expected: {bbox: [x, y, z]?, holes: [{x, y, z?, diameter?, counterbore_diameter?, through?, depth?}]?}")?;
        if !expected.is_object() {
            return Err("expected must be an object".into());
        }
        let tolerance = match arguments.get("tolerance_mm") {
            None => 0.6,
            Some(value) => value
                .as_f64()
                .filter(|t| *t > 0.0)
                .ok_or("tolerance_mm must be a positive number")?,
        };
        if self.attached_document_id.is_some() {
            self.refresh_read_only_snapshot()?;
        }
        let scene = self.manager.solid_scene_ref();
        let definitions = self.manager.hole_definitions();
        let built = summary::summarize(scene, &definitions);
        summary::check(&built, expected, tolerance)
    }

    /// A rectangular block through the ordinary sketch and extrude tools, so
    /// the result is editable history: an offset plane when the origin is off
    /// the XY plane, a dimensioned rectangle fixed at its origin corner, and
    /// one extrude. The parts are not traced separately.
    fn solid_box(&mut self, arguments: Value) -> Result<Value, String> {
        let triple = |key: &str, default: Option<[f64; 3]>| -> Result<[f64; 3], String> {
            match arguments.get(key) {
                None => default.ok_or_else(|| format!("solid_box requires {key}: [x, y, z]")),
                Some(value) => {
                    let list = value
                        .as_array()
                        .filter(|list| list.len() == 3)
                        .ok_or_else(|| format!("{key} must be [x, y, z]"))?;
                    let mut out = [0.0; 3];
                    for (i, entry) in list.iter().enumerate() {
                        out[i] = entry
                            .as_f64()
                            .ok_or_else(|| format!("{key} entries must be numbers"))?;
                    }
                    Ok(out)
                }
            }
        };
        let origin = triple("origin", Some([0.0; 3]))?;
        let size = triple("size", None)?;
        if size.iter().any(|s| *s <= 0.0) {
            return Err("size entries must be positive".into());
        }
        let operation = arguments
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("new_body");
        if !matches!(operation, "new_body" | "join" | "cut" | "intersect") {
            return Err("operation must be new_body, join, cut or intersect".into());
        }
        let target_body_ids = arguments
            .get("target_body_ids")
            .cloned()
            .unwrap_or_else(|| json!([]));
        if !target_body_ids.is_array() {
            return Err("target_body_ids must be an array of body ids".into());
        }
        let name = arguments
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Box {} x {} x {}", size[0], size[1], size[2]));
        self.composite_depth += 1;
        let result = self.solid_box_steps(origin, size, operation, target_body_ids, &name);
        self.composite_depth -= 1;
        result
    }

    fn solid_box_steps(
        &mut self,
        origin: [f64; 3],
        size: [f64; 3],
        operation: &str,
        target_body_ids: Value,
        name: &str,
    ) -> Result<Value, String> {
        let plane = if origin[2].abs() < 1e-9 {
            json!({"type":"origin_plane","plane":"xy"})
        } else {
            let planes = self.call_tool(
                "construction_plane_offset",
                json!({"name": format!("{name} base"), "reference": {"type":"origin_plane","plane":"xy"}, "distance": origin[2]}),
            )?;
            let datum_id = planes["planes"]
                .as_array()
                .and_then(|planes| planes.last())
                .and_then(|plane| plane["datum_id"].as_u64())
                .ok_or("offset plane did not return a datum_id")?;
            json!({"type":"datum_plane","datum_id":datum_id})
        };
        self.call_tool("sketch_begin", json!({"name": name, "plane": plane}))?;
        let rectangle = self.call_tool(
            "sketch_add_rectangle_locked",
            json!({"mode":"two_point","anchor":{"x":origin[0],"y":origin[1]},"corner_hint":{"x":origin[0]+size[0],"y":origin[1]+size[1]},"width_mm":size[0],"height_mm":size[1],"ctrl_held":true}),
        )?;
        let near =
            |value: &Value, wanted: f64| value.as_f64().is_some_and(|v| (v - wanted).abs() < 1e-6);
        let corner = rectangle["sketch"]["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entity| {
                entity["kind"] == "point"
                    && near(&entity["position"]["x"], origin[0])
                    && near(&entity["position"]["y"], origin[1])
            })
            .and_then(|entity| entity["id"].as_u64())
            .ok_or("rectangle origin corner not found in the sketch")?;
        self.call_tool(
            "sketch_add_constraint",
            json!({"type":"fix","entity":corner}),
        )?;
        self.call_tool("sketch_finish", json!({}))?;
        let update = self.call_tool(
            "solid_extrude",
            json!({"sketch_name":name,"profile_indices":[0],"operation":operation,"extent":{"type":"distance","distance":size[2]},"taper_angle_deg":0.0,"flip":false,"target_body_ids":target_body_ids}),
        )?;
        let feature_id = update["document"]["features"]
            .as_array()
            .and_then(|features| features.last())
            .and_then(|feature| feature["id"].as_u64());
        let body_ids: Vec<u64> = update["scene"]["bodies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|body| feature_id.is_none() || body["feature_id"].as_u64() == feature_id)
            .filter_map(|body| body["id"].as_u64())
            .collect();
        let scene = self.manager.solid_scene_ref();
        let definitions = self.manager.hole_definitions();
        Ok(json!({
            "feature_id": feature_id,
            "body_ids": body_ids,
            "sketch_name": name,
            "origin": origin,
            "size": size,
            "operation": operation,
            "summary": summary::summarize(scene, &definitions).json(false),
        }))
    }

    /// The print and plate size named by a print tool call.
    fn print_source(arguments: &Value) -> Result<(limo_cad_print::Source, f64, f64), String> {
        let path = arguments["path"]
            .as_str()
            .ok_or("print tools need path: an absolute PDF, PNG or PGM file of the print")?;
        let file = std::path::Path::new(path);
        if !file.is_absolute() {
            return Err("path must be absolute".into());
        }
        let page = match arguments.get("page") {
            None => 1,
            Some(page) => page
                .as_u64()
                .filter(|p| *p >= 1)
                .ok_or("page must be a positive integer")? as u32,
        };
        let length = arguments["length_mm"]
            .as_f64()
            .filter(|v| *v > 0.0)
            .ok_or("length_mm must be a positive number: the plate's plan-view length along x")?;
        let width = arguments["width_mm"]
            .as_f64()
            .filter(|v| *v > 0.0)
            .ok_or("width_mm must be a positive number: the plate's plan-view width along y")?;
        let source = limo_cad_print::Source::open(file, page)?;
        match arguments.get("hint") {
            None => limo_cad_print::set_hint(None),
            Some(hint) => {
                let text = hint
                    .as_str()
                    .ok_or("hint must be \"x0,y0,x1,y1\" page fractions around the plan view")?;
                limo_cad_print::set_hint(Some(limo_cad_print::parse_region(text)?));
            }
        }
        Ok((source, length, width))
    }

    /// Holes to draw or check on the print: the document's own holes (default),
    /// an explicit list, or none. `frame: "bbox_min"` shifts the document's
    /// holes so the bodies' lower-left corner is the print origin.
    fn print_holes(&mut self, arguments: &Value) -> Result<Vec<limo_cad_print::Hole>, String> {
        let explicit = match arguments.get("holes") {
            None => None,
            Some(Value::String(mode)) if mode == "document" => None,
            Some(Value::String(mode)) if mode == "none" => return Ok(Vec::new()),
            Some(Value::Array(list)) => Some(list.clone()),
            Some(_) => return Err("holes must be \"document\", \"none\" or a list of {x, y, diameter, counterbore_diameter?}".into()),
        };
        if let Some(list) = explicit {
            return list
                .iter()
                .map(|hole| {
                    Ok(limo_cad_print::Hole {
                        x: hole["x"].as_f64().ok_or("hole needs x")?,
                        y: hole["y"].as_f64().ok_or("hole needs y")?,
                        d: hole["diameter"].as_f64().unwrap_or(3.0),
                        cb: hole.get("counterbore_diameter").and_then(Value::as_f64),
                    })
                })
                .collect();
        }
        if self.attached_document_id.is_some() {
            self.refresh_read_only_snapshot()?;
        }
        let scene = self.manager.solid_scene_ref();
        let definitions = self.manager.hole_definitions();
        let built = summary::summarize(scene, &definitions);
        let (dx, dy) = match arguments.get("frame").and_then(Value::as_str) {
            None | Some("world") => (0.0, 0.0),
            Some("bbox_min") => built
                .bodies
                .iter()
                .fold((f64::INFINITY, f64::INFINITY), |(x, y), body| {
                    (x.min(body.min[0]), y.min(body.min[1]))
                }),
            Some(_) => return Err("frame must be world or bbox_min".into()),
        };
        let (dx, dy) = if dx.is_finite() { (dx, dy) } else { (0.0, 0.0) };
        Ok(built
            .holes
            .iter()
            .filter(|hole| hole.normal[2].abs() > 0.9)
            .map(|hole| limo_cad_print::Hole {
                x: hole.position[0] - dx,
                y: hole.position[1] - dy,
                d: hole.diameter,
                cb: hole.counterbore_diameter,
            })
            .collect())
    }

    fn print_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let (source, length, width) = Self::print_source(&arguments)?;
        let cal = limo_cad_print::calibrate(&source, length, width)?;
        let dpi = |default: u32| -> Result<u32, String> {
            match arguments.get("dpi") {
                None => Ok(default),
                Some(v) => v
                    .as_u64()
                    .filter(|d| (72..=1600).contains(d))
                    .map(|d| d as u32)
                    .ok_or("dpi must be between 72 and 1600".into()),
            }
        };
        let out_png = match arguments.get("out_png") {
            None => None,
            Some(v) => {
                let path = v.as_str().ok_or("out_png must be a path")?;
                if !std::path::Path::new(path).is_absolute() {
                    return Err("out_png must be an absolute path".into());
                }
                Some(path.to_owned())
            }
        };
        let write_png = |bytes: &[u8]| -> Result<Value, String> {
            match &out_png {
                Some(path) => {
                    std::fs::write(path, bytes).map_err(|e| format!("write {path}: {e}"))?;
                    Ok(json!(path))
                }
                None => Ok(Value::Null),
            }
        };
        let calibration: Value = serde_json::from_str(&cal.json()).map_err(|e| e.to_string())?;
        match name {
            "print_calibrate" => Ok(
                json!({"ok": true, "calibration": calibration, "length_mm": length, "width_mm": width}),
            ),
            "print_crop" => {
                let region = limo_cad_print::parse_region(
                    arguments["region"]
                        .as_str()
                        .ok_or("print_crop needs region: \"x0,y0,x1,y1\" in plate millimetres")?,
                )?;
                let grid = match arguments.get("grid_mm") {
                    None => 10.0,
                    Some(v) => v
                        .as_f64()
                        .filter(|g| *g >= 0.0)
                        .ok_or("grid_mm must be a non-negative number")?,
                };
                let holes = self.print_holes(&arguments)?;
                let image = limo_cad_print::crop(&source, &cal, region, dpi(400)?, &holes, grid)?;
                let png_path = write_png(&image.png)?;
                Ok(json!({
                    "ok": true,
                    "region_mm": [region.0, region.1, region.2, region.3],
                    "holes_drawn": holes.len(),
                    "legend": "red = model hole at its diameter, blue = counterbore, green ticks every grid_mm from the region corner (long tick and faint line every fifth)",
                    "png_path": png_path,
                    "image": {"png_base64": base64::engine::general_purpose::STANDARD.encode(&image.png), "width": image.width, "height": image.height, "dpi": image.dpi, "px_per_mm": image.px_per_mm},
                }))
            }
            "print_probe" => {
                let holes = self.print_holes(&arguments)?;
                let search = match arguments.get("search_mm") {
                    None => 2.5,
                    Some(v) => v
                        .as_f64()
                        .filter(|s| *s > 0.0)
                        .ok_or("search_mm must be positive")?,
                };
                let report = limo_cad_print::ring_score(&source, &cal, &holes, dpi(600)?, search)?;
                serde_json::from_str(&report).map_err(|e| e.to_string())
            }
            "print_symbols" => {
                let region = match arguments.get("region") {
                    None => None,
                    Some(v) => Some(limo_cad_print::parse_region(
                        v.as_str().ok_or("region must be \"x0,y0,x1,y1\"")?,
                    )?),
                };
                let holes = self.print_holes(&arguments)?;
                let draw = arguments
                    .get("draw")
                    .and_then(Value::as_bool)
                    .unwrap_or(out_png.is_some());
                let report =
                    limo_cad_print::symbols(&source, &cal, region, dpi(400)?, &holes, draw)?;
                let mut value: Value =
                    serde_json::from_str(&report.json).map_err(|e| e.to_string())?;
                if let Some(png) = report.png {
                    value["png_path"] = write_png(&png)?;
                    value["image"] = json!({"png_base64": base64::engine::general_purpose::STANDARD.encode(&png)});
                }
                Ok(value)
            }
            other => Err(format!("unknown print tool {other}")),
        }
    }

    fn wait_for_script_presentation(&mut self, initial: &Value) -> Result<(), String> {
        let mut result = initial.clone();
        loop {
            let state = &result["presentation"];
            if state["stopped"] == true {
                return Err("Playback stopped".into());
            }
            if state["step_pending"] == true {
                return Ok(());
            }
            let wait_ms = state["wait_ms"].as_u64().unwrap_or(0);
            if state["paused"] != true && wait_ms == 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(if state["paused"] == true {
                250
            } else {
                wait_ms.clamp(1, 250)
            }));
            result = self.call_tool(
                "cad_interface",
                json!({"action":"presentation","command":"status"}),
            )?;
            if result["status"] != "applied" {
                return Err(format!("Playback status failed: {result}"));
            }
        }
    }

    fn execute_interface(&mut self, arguments: &Value) -> Result<Value, String> {
        let name = arguments["operation"]
            .as_str()
            .ok_or("execute requires operation")?;
        let group = arguments["group"]
            .as_str()
            .ok_or("execute requires group from the catalog")?;
        if name == "cad_interface" || interface::group_for(name) != Some(group) {
            return Err("operation does not belong to the requested interface group".into());
        }
        let payload = arguments.get("arguments").cloned().unwrap_or(json!({}));
        if !is_modeling_mutate(name) || self.attached_document_id.is_none() {
            return self.call_tool(name, payload);
        }
        let session_id = self.attached_document_id.clone().unwrap();
        if session::heartbeat_meta(&session_id)["interface_version"] != 1 {
            return Err(json!({"code":"session_read_only","session_mode":"read_only_snapshot","session_id":session_id,"hint":"desktop does not support the grouped interface; rebuild/restart before executing (nothing submitted)","writeback":false}).to_string());
        }
        let generation = session::read_heartbeat_generation(&session_id)?;
        let model_change = changes_model(name);
        let submitted = self.submit_inbox_op(
            &json!({"name":name,"arguments":payload,"base_generation":generation}),
        )?;
        let seq = submitted["seq"]
            .as_u64()
            .ok_or("submission omitted sequence")?;
        let applied = await_playback_receipt(
            || {
                self.await_inbox_apply(&json!({"session_id":session_id,"seq":seq,"timeout_ms":30000,"refresh":model_change && !self.script_running,"poll_ms":10}))
            },
            || {
                session::request_ui(
                    &json!({"action":"presentation","command":"status"}),
                    Some(&session_id),
                )
            },
        )?;
        if applied["status"] != "applied" {
            return Err(applied.to_string());
        }
        if model_change && self.script_running && applied["model_published"] == true {
            self.live_snapshot_dirty = true;
        }

        if records_in_script(name) && applied["refreshed"] != true && self.composite_depth == 0 {
            self.tool_trace
                .push(json!({"name":name,"arguments":payload}));
        }
        let value: Value = serde_json::from_str(&session::read_session_file(
            &session_id,
            &format!("inbox/results/{seq}.json"),
        )?)
        .map_err(|e| e.to_string())?;
        if let Some(focus) = auto_focus_for_tool(name) {
            self.disclosure.auto_hint(focus);
        }
        let specs = tool_specs();
        let spec = specs.iter().find(|t| t.name == name).unwrap();
        Ok(annotate_disclosure(
            value,
            &self.disclosure,
            spec.pack,
            spec.spine,
        ))
    }

    /// Load `model.json` (+ optional `focus.json`) into this process.
    /// Marks attached only after a successful model load (Jack Â§3).
    /// Target by `session_id` (UUID), `window_id`, and/or `document_id`.
    fn attach_read_only_snapshot(&mut self, arguments: &Value) -> Result<Value, String> {
        let session_arg = arguments.get("session_id").and_then(Value::as_str);
        let window_arg = arguments.get("window_id").and_then(Value::as_str);
        let document_arg = arguments.get("document_id").and_then(Value::as_str);
        if writeback_requested(arguments) {
            let preview = session_arg.or(document_arg).or(window_arg);
            return Err(session_lock_error("writeback_rejected", preview));
        }
        let identity = session::resolve_attach_target(session_arg, window_arg, document_arg)?;
        let session_id = identity.session_id.as_str();
        self.load_snapshot_model(session_id, false)?;
        self.apply_snapshot_focus(session_id);
        self.attached_document_id = Some(session_id.to_string());
        self.failed_attachment = None;
        if let Some(binding) = &mut self.desktop_binding {
            binding.initial_selection_pending = false;
        }
        let heartbeat = session::heartbeat_meta(session_id);
        Ok(json!({
            "attached": true,
            "session_id": session_id,
            "window_id": identity.window_id,
            "document_id": identity.document_id.clone().unwrap_or_else(|| session_id.to_string()),
            "focus": self.disclosure.active().as_str(),
            "session_mode": if heartbeat["interface_version"] == 1 { "live" } else { "read_only_snapshot" },
            "writeback": false,
            "attached_generation": self.attached_generation,
            "heartbeat": heartbeat,
        }))
    }

    /// Re-read the currently attached session from disk into this process.
    fn refresh_read_only_snapshot(&mut self) -> Result<Value, String> {
        if let Some(session_id) = self
            .failed_attachment
            .as_ref()
            .map(|failure| failure.session_id.clone())
        {
            let mut result = self.attach_read_only_snapshot(&json!({"session_id":session_id}))?;
            result["refreshed"] = json!(true);
            return Ok(result);
        }
        let Some(session_id) = self.attached_document_id.clone() else {
            return Err("no session attached; call cad_attach first".to_string());
        };
        if let Err(error) = self.load_snapshot_model(&session_id, false) {
            self.failed_attachment = Some(AttachmentFailure {
                session_id,
                error: error.clone(),
            });
            return Err(error);
        }
        self.apply_snapshot_focus(&session_id);
        Ok(json!({
            "refreshed": true,
            "session_id": session_id,
            "focus": self.disclosure.active().as_str(),
            "session_mode": if session::heartbeat_meta(&session_id)["interface_version"] == 1 { "live" } else { "read_only_snapshot" },
            "writeback": false,
            "attached_generation": self.attached_generation,
        }))
    }

    /// Submit one modeling mutate into `inbox/<seq>.json`. Does not touch the
    /// MCP in-memory document; UI/engine applies, then `cad_refresh`.
    fn submit_inbox_op(&self, arguments: &Value) -> Result<Value, String> {
        let Some(session_id) = self.attached_document_id.clone() else {
            return Err(session::not_attached_error());
        };
        let name = arguments
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required argument 'name'".to_string())?;
        let op_arguments = arguments.get("arguments").cloned().unwrap_or(json!({}));
        let base_generation = arguments
            .get("base_generation")
            .and_then(Value::as_u64)
            .ok_or_else(|| "missing required argument 'base_generation'".to_string())?;
        if !tool_specs().iter().any(|spec| spec.name == name) {
            return Err(format!("unknown tool: {name}"));
        }
        if limo_cad_mcp_mutate::lookup_mutate(name).is_none() {
            return Err(serde_json::to_string(&json!({
                "code": "unsupported_inbox_mutate",
                "writeback": false,
                "session_mode": "ui_owned_apply",
                "session_id": session_id,
                "name": name,
                "hint": "cad_submit only accepts modeling mutates with a shared engine mapping; inspect/export/control stay direct tools",
            }))
            .unwrap_or_else(|_| "unsupported inbox mutate".to_string()));
        }
        let current = session::read_heartbeat_generation(&session_id)?;
        if current != base_generation {
            return Err(session::generation_conflict_error(
                &session_id,
                base_generation,
                Some(current),
            ));
        }
        let identity = session::session_identity(&session_id);
        let seq = session::write_inbox_op(
            &session_id,
            &session::InboxOp::unstamped(name.to_string(), op_arguments, base_generation)
                .with_identity(&identity)
                .with_script_progress(self.script_progress),
        )?;
        Ok(json!({
            "submitted": true,
            "seq": seq,
            "path": format!("inbox/{seq}.json"),
            "session_id": session_id,
            "window_id": identity.window_id,
            "document_id": identity.document_id,
            "session_mode": "ui_owned_apply",
            "writeback": false,
            "applied": false,
            "base_generation": base_generation,
            "hint": "UI/engine applies inbox via host::handle; pass this session_id and seq to cad_await_apply (or cad_refresh after publish)",
        }))
    }

    /// Wait for a submitted inbox seq's apply receipt + publisher snapshot.
    /// Optional `refresh` (default true) reloads a newly published completed
    /// model. Active-sketch-only snapshots are reported but not misrepresented
    /// as a model refresh.
    fn await_inbox_apply(&mut self, arguments: &Value) -> Result<Value, String> {
        let Some(attached_session_id) = self.interface_session().map(str::to_owned) else {
            return Err(session::not_attached_error());
        };
        let session_id = match arguments.get("session_id") {
            Some(value) => value
                .as_str()
                .ok_or("session_id must be a string")?
                .to_string(),
            None => attached_session_id.clone(),
        };
        let owns_attachment = session_id == attached_session_id;
        let seq = arguments
            .get("seq")
            .and_then(Value::as_u64)
            .ok_or_else(|| "missing required argument 'seq'".to_string())?;
        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(session::AWAIT_APPLY_DEFAULT_TIMEOUT_MS);
        let poll_ms = arguments
            .get("poll_ms")
            .and_then(Value::as_u64)
            .unwrap_or(session::AWAIT_APPLY_DEFAULT_POLL_MS);
        let refresh = arguments
            .get("refresh")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let mut result = session::await_inbox_apply(&session_id, seq, timeout_ms, poll_ms)?;
        let published = result
            .get("published")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let model_published = result
            .get("model_published")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let status = result.get("status").and_then(Value::as_str).unwrap_or("");
        if refresh && owns_attachment && status == "applied" && published && model_published {
            let active = result["active_session_id"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| session_id.clone());
            self.follow_interface_attachment(&mut result, active, SnapshotRefresh::Immediate)?;
            let refreshed = result["attached"] == true;
            result["refreshed"] = json!(refreshed);
            result["hint"] = json!(if refreshed {
                "UI applied and published; MCP in-memory snapshot refreshed from disk"
            } else {
                "UI already applied and published. Recover snapshot attachment with cad_attach/cad_refresh; do not resubmit this operation."
            });
        }
        if self.script_running && owns_attachment && result["project_replaced"] == true {
            return Err(
                "Active document changed during script playback; no later commands were submitted"
                    .into(),
            );
        }
        Ok(result)
    }

    /// Observe attached vs live publisher generation, heartbeat age/stale,
    /// pending inbox, and last apply receipt. Headless returns a clear
    /// `not_attached` status object (not an error).
    fn session_status(&self) -> Result<Value, String> {
        if let Some(failure) = &self.failed_attachment {
            let generation = (self.attached_document_id.as_deref()
                == Some(failure.session_id.as_str()))
            .then_some(self.attached_generation)
            .flatten();
            let mut status = session::session_status_json(&failure.session_id, generation)?;
            status["attached"] = json!(false);
            status["code"] = json!("attachment_unavailable");
            status["snapshot_error"] = json!(failure.error);
            status["model_commands_blocked"] = json!(true);
            status["retained_snapshot_session_id"] = json!(self.attached_document_id);
            status["retained_snapshot_generation"] = json!(self.attached_generation);
            status["stale"] = json!(true);
            status["hint"] = json!("Recover the active document with cad_attach or cad_refresh before sending model commands.");
            return Ok(status);
        }
        let Some(session_id) = self.attached_document_id.as_deref() else {
            return Ok(session::not_attached_status_json());
        };
        session::session_status_json(session_id, self.attached_generation)
    }

    /// Read the publication fence before the model, and commit it only after a
    /// successful load. UI acknowledgements can skip an identical snapshot but
    /// must still record its fence (e.g. a completed undo returns the same model).
    fn load_snapshot_model(
        &mut self,
        session_id: &str,
        skip_unchanged: bool,
    ) -> Result<bool, String> {
        let publication_generation = session::read_model_publication_generation(session_id);
        let model_json = session::require_model_json(session_id)?;
        if skip_unchanged && self.loaded_snapshot_json.as_deref() == Some(model_json.as_str()) {
            self.attached_generation = publication_generation;
            self.live_snapshot_dirty = false;
            return Ok(false);
        }
        let plan_value = parse_engine_envelope(host::handle(
            &mut self.manager,
            "project_prepare_load",
            &serde_json::to_string(&Value::String(model_json.clone()))
                .map_err(|e| e.to_string())?,
        ))?;
        let plan: RecomputePlanDto = serde_json::from_value(plan_value)
            .map_err(|error| format!("invalid model.json / recompute plan: {error}"))?;
        let transaction_id = plan.transaction_id;
        let queries = self.manager.history_support_queries();
        let (scene, verified) = match self.kernel.recompute_with_supports(&plan, &queries) {
            Ok(scene) => scene,
            Err(error) => {
                self.manager.cancel_solid_recompute(transaction_id);
                return Err(format!(
                    "session '{session_id}' model failed to recompute: {error}"
                ));
            }
        };
        self.manager
            .commit_solid_with_verified_supports(
                CommitKernelRequest {
                    transaction_id,
                    scene,
                },
                &verified,
            )
            .map_err(|e| e.to_string())?;

        self.seed_script_baseline_from_model(&model_json);
        self.loaded_snapshot_json = Some(model_json);
        self.attached_generation = publication_generation;
        self.live_snapshot_dirty = false;
        Ok(true)
    }

    /// Emit a version-1 `.limo.jsonc` from the last successful script source
    /// or, failing that, from this process `tool_trace` (lossy).
    ///
    /// `auto` keeps the authored script only while no modeling tool has
    /// succeeded since that run, traced or live. A later mutation makes it
    /// stale, and `auto` follows the session trace instead of labelling the
    /// old script lossless.
    fn export_script(&mut self, arguments: &Value) -> Result<Value, String> {
        let name = arguments
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Exported model");
        let from = arguments
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or("auto");
        if !matches!(from, "auto" | "last_script" | "session_trace") {
            return Err("export_script from must be auto, last_script, or session_trace".into());
        }
        let stale = self.last_script_source.is_some()
            && self.modeling_mutations != self.last_script_mutations;
        let use_authored = match from {
            "last_script" => self.last_script_source.is_some(),
            "auto" => self.last_script_source.is_some() && !stale,
            _ => false,
        };
        if use_authored {
            let mut notes = vec![
                "Source is the expanded JSONC last successfully run via action script in this process.",
                "Comments may be absent if includes were flattened; commands and refs match the replay.",
                "cad_script remains the forward MCP call dump â€” not this JSONC export.",
            ];
            if stale {
                notes.push(
                    "stale is true: tools ran after that script, so this source no longer matches the session. Ask for from:auto to export the session trace.",
                );
            }
            let source = self
                .last_script_source
                .clone()
                .ok_or("No last_script source in this process; run action script first")?;
            return script_export::export_script_result(source, "lossless_authored", stale, notes);
        }
        if from == "last_script" {
            return Err("No last_script source in this process; run action script first".into());
        }
        let source = script_export::session_trace_to_v1_source(&self.tool_trace, name)?;
        let mut notes = vec![
            "Built from this process tool_trace with literal arguments (no $select/$project, no notes).",
            "Prefer hand-authored JSONC for durable recipes; use this for scratch replay of a blank-session MCP build.",
            "cad_load_project_model attach baselines are omitted; UI-only history is not reverse-engineered.",
            "cad_script remains the forward MCP call dump â€” not this JSONC export.",
        ];
        if from == "auto" && stale {
            notes.insert(
                0,
                "The retained authored script is stale because tools ran after it. This export follows the session trace instead.",
            );
        }
        script_export::export_script_result(source, "lossy_session_trace", false, notes)
    }

    /// Clear `tool_trace` and seed `cad_load_project_model` with the loaded model JSON.
    fn seed_script_baseline_from_model(&mut self, model_json: &str) {
        self.last_script_source = None;
        self.last_script_mutations = self.modeling_mutations;
        self.tool_trace.clear();
        self.tool_trace.push(json!({
            "name": "cad_load_project_model",
            "arguments": { "model_json": model_json },
        }));
    }

    fn apply_snapshot_focus(&mut self, session_id: &str) {
        let Ok(focus_json) = session::read_session_file(session_id, "focus.json") else {
            return;
        };
        let Ok(focus_value) = serde_json::from_str::<Value>(&focus_json) else {
            return;
        };
        if let Some(focus_name) = focus_value.get("focus").and_then(Value::as_str) {
            if let Some(focus) = FocusPack::parse(focus_name) {
                self.disclosure.set_focus(focus, false);
                self.disclosure.clear_explicit_lock();
            }
        }
    }

    fn export_mesh(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        if !self.manager.solid_scene_ref().errors.is_empty() {
            return Err("Resolve timeline errors before exporting mesh files.".to_string());
        }
        let request: MeshExportRequest = if arguments.is_null() {
            MeshExportRequest::default()
        } else {
            serde_json::from_value(arguments)
                .map_err(|error| format!("bad mesh export arguments: {error}"))?
        };
        if request.expected_model_json.is_some() {
            request
                .check_model_snapshot(
                    &self
                        .manager
                        .export_project_model()
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
        }
        let scene = self.manager.solid_scene_ref();
        scene.require_complete_display_mesh(&request.body_ids)?;
        let appearances = self.manager.body_appearances();
        let mut meshes = self
            .kernel
            .tessellate_bodies(&request)
            .map_err(|error| error.to_string())?;
        for mesh in &mut meshes {
            if let Some(body) = scene.bodies.iter().find(|body| body.id == mesh.body_id) {
                mesh.name = body.name.clone();
            }
        }
        if request.scope == limo_cad_export::MeshExportScope::Definition
            && request
                .named_view
                .as_deref()
                .is_some_and(|name| !name.is_empty())
        {
            return Err("Named-view placement requires assembly scope.".into());
        }
        let solution = if request.scope == limo_cad_export::MeshExportScope::Definition {
            self.manager.assembly_solution()
        } else {
            self.manager
                .export_view_solution(request.named_view.as_deref())
                .map_err(|e| e.to_string())?
        };
        if request.scope == limo_cad_export::MeshExportScope::Assembly && !solution.solved {
            return Err("Resolve assembly errors before mesh export.".into());
        }
        let instances: Vec<_> = solution
            .instance_body_poses
            .iter()
            .map(|p| limo_cad_export::MeshInstance {
                body_id: p.body_id,
                occurrence_id: p.occurrence_id.0,
                translation: p.translation,
                rotation: p.rotation,
                visible: p.visible,
            })
            .collect();
        let portable_scene = name == "solid_export_3mf"
            && request.scope == limo_cad_export::MeshExportScope::Assembly;
        let bytes = if portable_scene {
            limo_cad_export::write_3mf_scene(
                &meshes,
                &appearances,
                &request,
                &self.manager.assembly_document_ref().component_structure,
                &solution,
            )
            .map_err(|e| e.to_string())?
        } else {
            let meshes = limo_cad_export::prepare_export_meshes(&meshes, &instances, request.scope)
                .map_err(|e| e.to_string())?;
            if name == "solid_export_stl" {
                limo_cad_export::write_stl(&meshes).map_err(|error| error.to_string())?
            } else {
                limo_cad_export::ExportFacade::export_3mf(&meshes, &appearances, &request)
                    .map_err(|error| error.to_string())?
            }
        };
        Ok(json!({
            "format": if name == "solid_export_stl" { "stl" } else { "3mf" },
            "encoding": "base64",
            "slicer_target": request.slicer_target,
            "byte_length": bytes.len(),
            "bytes_base64": BASE64.encode(bytes),
        }))
    }

    fn tessellate_tool(&mut self, arguments: Value) -> Result<Value, String> {
        if !self.manager.solid_scene_ref().errors.is_empty() {
            return Err("Resolve timeline errors before tessellating.".to_string());
        }
        let request: MeshExportRequest = if arguments.is_null() {
            MeshExportRequest::default()
        } else {
            serde_json::from_value(arguments)
                .map_err(|error| format!("bad tessellate arguments: {error}"))?
        };
        let scene = self.manager.solid_scene_ref();
        scene.require_complete_display_mesh(&request.body_ids)?;
        let mut meshes = self
            .kernel
            .tessellate_bodies(&request)
            .map_err(|error| error.to_string())?;
        for mesh in &mut meshes {
            if let Some(body) = scene.bodies.iter().find(|body| body.id == mesh.body_id) {
                mesh.name = body.name.clone();
            }
        }
        let bodies: Vec<Value> = meshes
            .iter()
            .map(|mesh| {
                let mut min = [f64::MAX; 3];
                let mut max = [f64::MIN; 3];
                for p in mesh.positions.as_chunks::<3>().0 {
                    for i in 0..3 {
                        min[i] = min[i].min(p[i]);
                        max[i] = max[i].max(p[i]);
                    }
                }
                json!({
                    "body_id": mesh.body_id.0,
                    "name": mesh.name,
                    "triangle_count": mesh.triangle_count(),
                    "vertex_count": mesh.positions.len() / 3,
                    "bbox_min": min,
                    "bbox_max": max,
                })
            })
            .collect();
        Ok(json!({
            "linear_deflection": request.linear_deflection,
            "angular_deflection": request.angular_deflection,
            "body_count": bodies.len(),
            "bodies": bodies,
        }))
    }

    fn export_preflight_tool(&mut self, arguments: Value) -> Result<Value, String> {
        let request: MeshExportRequest = serde_json::from_value(if arguments.is_null() {
            json!({})
        } else {
            arguments
        })
        .map_err(|e| e.to_string())?;
        if request.scope != limo_cad_export::MeshExportScope::Assembly {
            return Err("Print layout checks require assembly scope.".into());
        }
        request
            .check_model_snapshot(
                &self
                    .manager
                    .export_project_model()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        let scene = self.manager.solid_scene_ref();
        let errors: Vec<String> = scene
            .errors
            .iter()
            .map(|error| format!("feature {}: {}", error.feature_id.0, error.message))
            .collect();
        let body_ids: Vec<u64> = scene.bodies.iter().map(|body| body.id.0).collect();
        let appearances = self.manager.body_appearances();
        let appearing: Vec<u64> = appearances.iter().map(|a| a.body_id.0).collect();
        let missing_appearance: Vec<u64> = body_ids
            .iter()
            .copied()
            .filter(|id| !appearing.contains(id))
            .collect();
        let ok = errors.is_empty() && !body_ids.is_empty();
        let mut result = json!({
            "ok": ok,
            "body_count": body_ids.len(),
            "body_ids": body_ids,
            "timeline_errors": errors,
            "appearances_assigned": appearing.len(),
            "bodies_missing_appearance": missing_appearance,
            "hints": if !ok {
                json!([
                    "Fix timeline_errors before export.",
                    "Empty documents cannot export meshes.",
                    "Optional: set_body_appearance / material_catalog for colored 3MF."
                ])
            } else {
                json!([
                    "Ready for solid_export_3mf (preferred) or solid_export_stl / solid_export_step."
                ])
            },
        });
        result["print_intent"] = serde_json::to_value(
            self.manager
                .effective_print_intent(
                    request.body_ids.clone(),
                    Some(limo_cad_core::PrintIntentTargetDto::Portable),
                )
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if ok {
            self.manager
                .solid_scene_ref()
                .require_complete_display_mesh(&request.body_ids)?;
            let meshes = self
                .kernel
                .tessellate_bodies(&request)
                .map_err(|e| e.to_string())?;
            let solution = self
                .manager
                .export_view_solution(request.named_view.as_deref())
                .map_err(|e| e.to_string())?;
            let bed = match request.print_bed {
                Some(bed) => bed,
                None => self
                    .manager
                    .export_print_bed(request.named_view.as_deref())
                    .map_err(|e| e.to_string())?,
            };
            let layout = limo_cad_export::analyze_print_layout(
                &meshes,
                &self.manager.assembly_document_ref().component_structure,
                &solution,
                &bed,
            )
            .map_err(|e| e.to_string())?;
            if layout.printable_instances == 0 {
                result["ok"] = json!(false);
                result["hints"] = json!(["No visible printable instances. Select another view or restore visibility before export."]);
            }
            let effective = self
                .manager
                .effective_print_intent(
                    request.body_ids.clone(),
                    Some(limo_cad_core::PrintIntentTargetDto::Portable),
                )
                .map_err(|e| e.to_string())?;
            result["manufacturing"] = serde_json::to_value(
                limo_cad_export::manufacturing_report::manufacturing_preflight_report(
                    &meshes,
                    &appearances,
                    &self.manager.assembly_document_ref().component_structure,
                    &solution,
                    &self.manager.print_intent(),
                    &effective,
                )
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            result["layout"] = serde_json::to_value(layout).map_err(|e| e.to_string())?;
        }
        Ok(result)
    }

    fn demo_pip_3mf_tool(&mut self, arguments: Value) -> Result<Value, String> {
        let request: MeshExportRequest = if arguments.is_null() {
            MeshExportRequest::default()
        } else {
            serde_json::from_value(arguments.clone())
                .map_err(|error| format!("bad demo export arguments: {error}"))?
        };
        let kind = arguments
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("cam_bolt");
        let (meshes, appearances, demo) = match kind {
            "clip" | "latch" => {
                let (m, a) = limo_cad_export::print_in_place_clip();
                (m, a, "print_in_place_clip")
            }
            "cam_bolt" | "cam" => {
                let (m, a) = limo_cad_export::print_in_place_cam_bolt();
                (m, a, "print_in_place_cam_bolt")
            }
            other => {
                return Err(format!(
                    "unknown demo kind '{other}' (expected cam_bolt or clip)"
                ))
            }
        };
        let bytes = limo_cad_export::ExportFacade::export_3mf(&meshes, &appearances, &request)
            .map_err(|error| error.to_string())?;
        Ok(json!({
            "format": "3mf",
            "encoding": "base64",
            "demo": demo,
            "body_count": meshes.len(),
            "clearance_mm": limo_cad_export::CLEAR_MM,
            "slicer_target": request.slicer_target,
            "byte_length": bytes.len(),
            "bytes_base64": BASE64.encode(bytes),
        }))
    }
}

fn parse_engine_envelope(raw: String) -> Result<Value, String> {
    let envelope: Value =
        serde_json::from_str(&raw).map_err(|error| format!("invalid engine response: {error}"))?;
    if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(envelope.get("value").cloned().unwrap_or(Value::Null))
    } else {
        Err(envelope
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown Limo CAD engine error")
            .to_string())
    }
}

fn annotate_disclosure(
    mut value: Value,
    disclosure: &DisclosureState,
    pack: FocusPack,
    spine: bool,
) -> Value {
    let note = disclosure.disclosure_note(pack, spine);

    if let Value::Object(object) = &mut value {
        object.insert("_disclosure".to_string(), note);
    }
    value
}

fn tool_entry(tool: &ToolSpec) -> Value {
    json!({
        "name": tool.name,
        "title": tool.title,
        "description": tool.description,
        "inputSchema": tool.input_schema,
        "_meta": {"group":interface::group_for(tool.name)}
    })
}

fn full_tool_catalog() -> Value {
    Value::Array(
        tool_specs()
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "group": interface::group_for(tool.name),
                    "title": tool.title,
                    "description": tool.description,
                    "inputSchema": tool.input_schema,
                    "execution": match tool.execution {
                        Execution::Direct => "direct",
                        Execution::SolidReplay => "solid_replay",
                        Execution::Control => "control",
                    },
                    "mutates": changes_model(tool.name),
                    "spine": tool.spine,
                })
            })
            .collect(),
    )
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn named_view_schema() -> Value {
    let vector = json!({"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3});
    object_schema(
        json!({
            "name": {"type":"string","minLength":1,"maxLength":200},
            "camera": object_schema(json!({"position":vector,"target":vector,"up":vector}), &["position", "target", "up"]),
            "visible_body_ids": {"type":"array","items":{"type":"integer","minimum":1}},
            "print_layout": {"type":"boolean","default":false},
            "print_bed": print_bed_schema(),
            "occurrence_offsets": {"type":"array","items":occurrence_offset_schema()},
            "part_offsets": {"type":"array","items":object_schema(json!({
                "body_id":{"type":"integer","minimum":1},"translation":vector
            }), &["body_id", "translation"])}
        }),
        &["name", "camera", "visible_body_ids"],
    )
}

fn print_bed_schema() -> Value {
    object_schema(
        json!({"name":{"type":"string"},"size_mm":{"type":"array","items":{"type":"number","exclusiveMinimum":0},"minItems":3,"maxItems":3},"margin_mm":{"type":"number","minimum":0},"nozzle_mode":{"type":"string","enum":["main","dual"]}, "origin_mm":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}, "printable_regions":{"type":"array","items":print_region_schema()}, "excluded_regions":{"type":"array","items":print_region_schema()}, "source":print_profile_source_schema()}),
        &["name", "size_mm"],
    )
}
fn occurrence_offset_schema() -> Value {
    object_schema(
        json!({"occurrence_id":{"type":"integer","minimum":1},"translation":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"rotation":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4}}),
        &["occurrence_id", "translation"],
    )
}

fn print_region_schema() -> Value {
    json!({"type":"array","minItems":3,"maxItems":1024,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}})
}
fn print_profile_source_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["repository","revision","profile","files"],"properties":{"repository":{"type":"string"},"revision":{"type":"string"},"profile":{"type":"string"},"files":{"type":"object","additionalProperties":{"type":"string"}}}})
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

/// Shared input schema of the print tools: the print file, the plate size, and
/// where the holes to draw or check come from.
fn print_schema(extra: Value) -> Value {
    let mut properties = json!({
        "path": {"type":"string","description":"Absolute path of the print: PDF (rendered through pdftoppm), PNG or PGM"},
        "page": {"type":"integer","minimum":1,"default":1},
        "length_mm": {"type":"number","exclusiveMinimum":0,"description":"Plate length along x in the plan view"},
        "width_mm": {"type":"number","exclusiveMinimum":0,"description":"Plate width along y in the plan view"},
        "hint": {"type":"string","description":"Optional \"x0,y0,x1,y1\" page fractions (top-left origin) around the plan view when the automatic outline search picks another rectangle"},
        "holes": {"description":"\"document\" (default: the current document's holes in world x, y), \"none\", or a list of {x, y, diameter, counterbore_diameter?} in plate mm"},
        "frame": {"type":"string","enum":["world","bbox_min"],"default":"world","description":"With bbox_min the document's holes are shifted so the bodies' lower-left corner is the print origin"}
    });
    for (key, value) in extra.as_object().into_iter().flatten() {
        properties[key] = value.clone();
    }
    object_schema(properties, &["path", "length_mm", "width_mm"])
}

fn object_or_null(schema: Value) -> Value {
    json!({ "oneOf": [schema, { "type": "null" }] })
}

fn dto_schema(description: &str) -> Value {
    json!({
        "type": "object",
        "description": description,
        "additionalProperties": true
    })
}

fn point_schema() -> Value {
    object_schema(
        json!({
            "x": { "type": "number", "description": "Sketch-local X coordinate in millimeters." },
            "y": { "type": "number", "description": "Sketch-local Y coordinate in millimeters." }
        }),
        &["x", "y"],
    )
}

fn entity_ids_schema() -> Value {
    json!({
        "type": "array",
        "items": { "type": "integer", "minimum": 1 },
        "minItems": 1
    })
}

/// Tools allowed to run in-process while snapshot-attached (#55 list).
/// `cad_submit` is the mutate path: only tools *not* on this list.
fn is_read_safe_while_attached(name: &str) -> bool {
    if matches!(
        name,
        "drawing_document" | "drawing_projection" | "drawing_export" | "solid_section_review"
    ) {
        return true;
    }
    matches!(
        name,
        "cad_get_focus"
            | "cad_set_focus"
            | "cad_list_focus_areas"
            | "cad_get_tool_disclosure_mode"
            | "cad_set_tool_disclosure_mode"
            | "cad_list_all_tools"
            | "cad_help"
            | "cad_cancel_recompute"
            | "cad_list_sessions"
            | "cad_computer_control"
            | "cad_route"
            | "cad_interface"
            | "cad_attach"
            | "cad_refresh"
            | "cad_detach"
            | "cad_submit"
            | "cad_await_apply"
            | "cad_session_status"
            | "cad_document"
            | "cad_project_model"
            | "cad_compare_solids"
            | "project_visibility"
            | "named_views"
            | "named_view_solution"
            | "print_intent_get"
            | "print_intent_height_binding"
            | "print_intent_effective"
            | "print_modifier_effective"
            | "bambu_template_inspect"
            | "bambu_local_verification_start"
            | "bambu_local_verification_poll"
            | "bambu_local_verification_cancel"
            | "bambu_project_preview"
            | "solid_export_bambu_project"
            | "sketch_active"
            | "sketch_finished"
            | "sketch_profiles"
            | "sketch_preview_line"
            | "sketch_preview_line_locked"
            | "sketch_preview_fillet"
            | "sketch_preview_offset"
            | "sketch_preview_trim"
            | "sketch_eval_expression"
            | "construction_plane_definitions"
            | "solid_scene"
            | "assembly_document"
            | "assembly_solution"
            | "assembly_interference_check"
            | "assembly_swept_collision_check"
            | "assembly_preview_joint_coordinates"
            | "assembly_preview_mechanism_drag"
            | "assembly_evaluate_motion_study"
            | "assembly_sample_motion_study"
            | "assembly_export_motion_path_csv"
            | "solid_tessellate"
            | "solid_extrude_definitions"
            | "solid_revolve_definitions"
            | "solid_sweep_definitions"
            | "solid_loft_definitions"
            | "solid_rib_definitions"
            | "solid_fillet_definitions"
            | "solid_chamfer_definitions"
            | "solid_hole_definitions"
            | "solid_body_feature_definitions"
            | "solid_export_step"
            | "solid_export_stl"
            | "solid_export_3mf"
            | "solid_export_preflight"
            | "demo_export_pip_3mf"
            | "printer_catalog"
            | "material_catalog"
            | "body_appearances"
            | "cam_get_document"
            | "cam_toolpath_statuses"
            | "print_calibrate"
            | "print_crop"
            | "print_probe"
            | "print_symbols"
            | "solid_box"
    )
}

fn is_modeling_mutate(name: &str) -> bool {
    limo_cad_mcp_mutate::is_inbox_mutate(name)
}

fn changes_model(name: &str) -> bool {
    limo_cad_mcp_mutate::lookup_mutate(name).is_some_and(|spec| !spec.is_read_only())
}

fn writeback_requested(arguments: &Value) -> bool {
    match arguments.get("writeback") {
        None => false,
        Some(Value::Bool(false)) => false,
        Some(_) => true,
    }
}

fn session_lock_error(code: &str, session_id: Option<&str>) -> String {
    serde_json::to_string(&json!({
        "code": code,
        "writeback": false,
        "session_mode": "read_only_snapshot",
        "session_id": session_id,
        "hint": "cad_submit for mutates while attached; cad_await_apply after submit; cad_session_status for attach vs live generation; cad_refresh to re-read UI; cad_detach to fork headless"
    }))
    .unwrap_or_else(|_| {
        format!(
            "{{\"code\":\"{code}\",\"writeback\":false,\"session_mode\":\"read_only_snapshot\"}}"
        )
    })
}

fn gear_relation_schema(update: bool) -> Value {
    let mut properties = json!({
        "name": { "type": "string", "minLength": 1 },
        "joint_a": { "type": "integer", "minimum": 1 },
        "joint_b": { "type": "integer", "minimum": 1 },
        "teeth_a": { "type": "integer", "minimum": 1, "maximum": 4294967295_u64 },
        "teeth_b": { "type": "integer", "minimum": 1, "maximum": 4294967295_u64 },
        "reverse": { "type": "boolean", "default": true, "description": "True for an external pair turning in opposite directions." },
        "phase_deg": { "type": "number", "default": 0.0, "description": "Unwrapped relation: angle_b = phase_deg + (reverse ? -1 : 1) * teeth_a / teeth_b * angle_a. Connector frames define angular zero." }
    });
    let mut required = vec!["name", "joint_a", "joint_b", "teeth_a", "teeth_b"];
    if update {
        properties["id"] = json!({ "type": "integer", "minimum": 1 });
        required.push("id");
    }
    object_schema(properties, &required)
}

/// Calls borrow immutable tool schemas constructed once per process.
fn tool_specs() -> &'static [ToolSpec] {
    static SPECS: std::sync::OnceLock<Vec<ToolSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(build_tool_specs)
}

fn build_tool_specs() -> Vec<ToolSpec> {
    let point = point_schema();
    let entity_ids = entity_ids_schema();
    let plane = json!({
        "oneOf": [
            {
                "type": "object",
                "properties": {
                    "type": { "const": "origin_plane" },
                    "plane": { "type": "string", "enum": ["xy", "xz", "yz"] }
                },
                "required": ["type", "plane"],
                "additionalProperties": false
            },
            {
                "type": "object",
                "properties": {
                    "type": { "const": "planar_face" },
                    "face_id": { "type": "integer", "minimum": 1 }
                },
                "required": ["type", "face_id"],
                "additionalProperties": false
            },
            {
                "type": "object",
                "properties": {
                    "type": { "const": "datum_plane" },
                    "datum_id": { "type": "integer", "minimum": 1 }
                },
                "required": ["type", "datum_id"],
                "additionalProperties": false
            }
        ]
    });
    let point3 = object_schema(
        json!({
            "x": { "type": "number" },
            "y": { "type": "number" },
            "z": { "type": "number" }
        }),
        &["x", "y", "z"],
    );
    let profile_indices = json!({
        "type": "array",
        "items": { "type": "integer", "minimum": 0 },
        "minItems": 1,
        "uniqueItems": true
    });
    let body_ids = json!({
        "type": "array",
        "items": { "type": "integer", "minimum": 1 },
        "uniqueItems": true
    });
    let edge_ids = json!({
        "type": "array",
        "items": { "type": "integer", "minimum": 1 },
        "minItems": 1,
        "uniqueItems": true
    });
    let extrude = object_schema(
        json!({
            "sketch_name": { "type": "string", "minLength": 1 },
            "profile_indices": profile_indices.clone(),
            "operation": { "type": "string", "enum": ["new_body", "join", "cut", "intersect"] },
            "extent": {
                "type": "object",
                "description": "Tagged extent: distance, two_sides, symmetric, through_all, or to_face.",
                "additionalProperties": true
            },
            "taper_angle_deg": { "type": "number", "exclusiveMinimum": -89, "exclusiveMaximum": 89 },
            "flip": { "type": "boolean" },
            "target_body_ids": body_ids.clone()
        }),
        &[
            "sketch_name",
            "profile_indices",
            "operation",
            "extent",
            "taper_angle_deg",
            "flip",
            "target_body_ids",
        ],
    );
    let revolve = object_schema(
        json!({
            "sketch_name": { "type": "string", "minLength": 1 },
            "profile_indices": profile_indices,
            "axis_origin": point.clone(),
            "axis_direction": point.clone(),
            "axis_line_sketch_name": { "type": ["string", "null"], "minLength": 1, "description": "Optional sketch owning the stable axis line. It must be coplanar with sketch_name; omitted values use sketch_name." },
            "axis_line_entity_id": { "type": ["integer", "null"], "minimum": 1, "description": "Optional stable line entity id; overrides the manual axis." },
            "angle_deg": { "type": "number", "exclusiveMinimum": 0, "maximum": 360 },
            "flip": { "type": "boolean" },
            "operation": { "type": "string", "enum": ["new_body", "join", "cut", "intersect"] },
            "target_body_ids": body_ids.clone()
        }),
        &[
            "sketch_name",
            "profile_indices",
            "axis_origin",
            "axis_direction",
            "angle_deg",
            "flip",
            "operation",
            "target_body_ids",
        ],
    );
    let profile_ref = object_schema(
        json!({
            "sketch_name": { "type": "string", "minLength": 1 },
            "profile_index": { "type": "integer", "minimum": 0 }
        }),
        &["sketch_name", "profile_index"],
    );
    let solid_operation =
        json!({ "type": "string", "enum": ["new_body", "join", "cut", "intersect"] });
    let path_ref = object_schema(
        json!({
            "sketch_name": { "type": "string", "minLength": 1 },
            "entity_ids": entity_ids.clone()
        }),
        &["sketch_name", "entity_ids"],
    );
    let sweep = object_schema(
        json!({
            "profile": profile_ref.clone(),
            "path_sketch_name": { "type": "string", "minLength": 1 },
            "path_entity_ids": entity_ids.clone(),
            "operation": solid_operation.clone(),
            "target_body_ids": body_ids.clone(),
            "guide_rail": { "oneOf": [path_ref.clone(), {"type": "null"}] },
            "orientation": { "type": "string", "enum": ["corrected_frenet", "frenet", "fixed"] },
            "transition": { "type": "string", "enum": ["transformed", "right_corner", "round_corner"] },
            "force_c1": { "type": "boolean" }
        }),
        &[
            "profile",
            "path_sketch_name",
            "path_entity_ids",
            "operation",
            "target_body_ids",
        ],
    );
    let loft = object_schema(
        json!({
            "sections": { "type": "array", "items": profile_ref, "minItems": 2 },
            "ruled": { "type": "boolean" },
            "operation": solid_operation.clone(),
            "target_body_ids": body_ids.clone(),
            "continuity": { "type": "string", "enum": ["g0", "g1", "g2"] },
            "centerline": { "oneOf": [path_ref.clone(), {"type": "null"}] },
            "guide_rail": { "oneOf": [path_ref, {"type": "null"}] }
        }),
        &["sections", "ruled", "operation", "target_body_ids"],
    );
    let solid_fillet = object_schema(
        json!({
            "body_id": { "type": "integer", "minimum": 1 },
            "edge_ids": edge_ids.clone(),
            "radius": { "type": "number", "exclusiveMinimum": 0 },
            "tangent_chain": { "type": "boolean" }
        }),
        &["body_id", "edge_ids", "radius", "tangent_chain"],
    );
    let solid_chamfer = object_schema(
        json!({
            "body_id": { "type": "integer", "minimum": 1 },
            "edge_ids": edge_ids,
            "distance": { "type": "number", "exclusiveMinimum": 0 },
            "tangent_chain": { "type": "boolean" }
        }),
        &["body_id", "edge_ids", "distance", "tangent_chain"],
    );
    let sketch_point_reference = {
        let variants = ["point", "start", "end", "center"]
            .into_iter()
            .map(|kind| {
                object_schema(
                    json!({
                        "sketch_name": { "type": "string", "minLength": 1 },
                        "entity_id": { "type": "integer", "minimum": 1 },
                        "kind": { "const": kind }
                    }),
                    &["sketch_name", "entity_id", "kind"],
                )
            })
            .chain(std::iter::once(object_schema(
                json!({
                    "sketch_name": { "type": "string", "minLength": 1 },
                    "entity_id": { "type": "integer", "minimum": 1 },
                    "kind": { "const": "fit_point" },
                    "index": { "type": "integer", "minimum": 0 }
                }),
                &["sketch_name", "entity_id", "kind", "index"],
            )))
            .collect::<Vec<_>>();
        json!({ "oneOf": variants })
    };
    let hole_position = object_schema(
        json!({
            "position": point.clone(),
            "position_reference": {
                "oneOf": [sketch_point_reference.clone(), {"type": "null"}]
            }
        }),
        &["position"],
    );
    let hole_thread = object_schema(
        json!({
            "standard": { "type": "string", "enum": ["iso_metric", "unified_inch", "custom_trapezoidal"] },
            "series": {
                "type": "string",
                "enum": ["metric_coarse", "metric_fine", "unc", "unf", "rounded"]
            },
            "designation": { "type": "string", "minLength": 1 },
            "class": { "type": "string", "minLength": 1 },
            "nominal_diameter": {
                "type": "number",
                "exclusiveMinimum": 0,
                "description": "Basic thread major diameter in millimetres."
            },
            "pitch": {
                "type": "number",
                "exclusiveMinimum": 0,
                "description": "Axial pitch in millimetres, including for Unified threads."
            },
            "threads_per_inch": {
                "type": ["number", "null"],
                "exclusiveMinimum": 0
            },
            "hand": { "type": "string", "enum": ["right", "left"] },
            "depth": {
                "type": ["number", "null"],
                "exclusiveMinimum": 0,
                "description": "Null threads the full cylindrical hole depth."
            },
            "representation": {
                "type": "string",
                "enum": ["modeled", "simplified"]
            },
            "tap_drill_designation": { "type": ["string", "null"] },
            "rounded_profile": {
                "description": "Required only for custom_trapezoidal / rounded / custom class. Native single-start 30-degree profile with circular root/crest rounds, NOT an ISO Tr fit. Both mating parts use identical nominal/profile values; radial clearance enlarges only female radii, axial clearance enlarges its groove by the total given amount.",
                "oneOf": [object_schema(json!({
                    "radial_depth": {"type":"number", "exclusiveMinimum":0},
                    "corner_radius": {"type":"number", "exclusiveMinimum":0},
                    "radial_clearance": {"type":"number", "minimum":0},
                    "axial_clearance": {"type":"number", "minimum":0}
                }), &["radial_depth", "corner_radius", "radial_clearance", "axial_clearance"]), {"type":"null"}]
            }
        }),
        &[
            "standard",
            "series",
            "designation",
            "class",
            "nominal_diameter",
            "pitch",
            "threads_per_inch",
            "hand",
            "depth",
            "representation",
        ],
    );
    let external_thread = object_schema(
        json!({
            "body_id": {"type":"integer", "minimum":1},
            "face_id": {"type":"integer", "minimum":1},
            "thread": hole_thread.clone(),
            "flip": {"type":"boolean"}
        }),
        &["body_id", "face_id", "thread"],
    );
    let hole = object_schema(
        json!({
            "body_id": { "type": "integer", "minimum": 1 },
            "face_id": { "type": "integer", "minimum": 1 },
            "position": point.clone(),
            "position_reference": {
                "oneOf": [sketch_point_reference, {"type": "null"}]
            },
            "positions": {
                "type": "array",
                "items": hole_position,
                "minItems": 1
            },
            "diameter": { "type": "number", "exclusiveMinimum": 0 },
            "extent": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "type": { "const": "distance" },
                            "depth": { "type": "number", "exclusiveMinimum": 0 }
                        },
                        "required": ["type", "depth"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": { "type": { "const": "through_all" } },
                        "required": ["type"],
                        "additionalProperties": false
                    }
                ]
            },
            "style": { "type": "string", "enum": ["simple", "counterbore", "countersink"] },
            "counterbore_diameter": { "type": "number", "minimum": 0 },
            "counterbore_depth": { "type": "number", "minimum": 0 },
            "countersink_diameter": { "type": "number", "minimum": 0 },
            "countersink_angle_deg": { "type": "number", "exclusiveMinimum": 0, "exclusiveMaximum": 180 },
            "bottom_style": { "type": "string", "enum": ["flat", "drill_point"] },
            "drill_point_angle_deg": { "type": "number", "exclusiveMinimum": 0, "exclusiveMaximum": 180 },
            "thread": {
                "oneOf": [hole_thread, {"type": "null"}],
                "description": "Optional ISO metric or ASME B1.1 Unified internal thread. Hole diameter is the predrill diameter."
            },
            "flip": { "type": "boolean" }
        }),
        &[
            "body_id",
            "face_id",
            "position",
            "diameter",
            "extent",
            "style",
            "counterbore_diameter",
            "counterbore_depth",
            "countersink_diameter",
            "countersink_angle_deg",
            "flip",
        ],
    );
    let rib = object_schema(
        json!({
            "sketch_name": { "type": "string", "minLength": 1 },
            "line_entity_ids": entity_ids,
            "thickness": { "type": "number", "exclusiveMinimum": 0 },
            "depth": { "type": "number", "exclusiveMinimum": 0 },
            "extent": {
                "type": "object",
                "description": "Tagged Rib extent: distance, to_next, to_face, or through_all.",
                "additionalProperties": true
            },
            "symmetric": { "type": "boolean" },
            "flip": { "type": "boolean" },
            "operation": solid_operation,
            "target_body_ids": body_ids.clone()
        }),
        &[
            "sketch_name",
            "line_entity_ids",
            "thickness",
            "depth",
            "symmetric",
            "flip",
            "operation",
            "target_body_ids",
        ],
    );
    let face_ids = json!({
        "type": "array",
        "items": { "type": "integer", "minimum": 1 },
        "minItems": 1,
        "uniqueItems": true
    });
    let shell = object_schema(
        json!({
            "body_id": { "type": "integer", "minimum": 1 },
            "face_ids": face_ids,
            "thickness": { "type": "number", "exclusiveMinimum": 0 },
            "inward": { "type": "boolean" }
        }),
        &["body_id", "face_ids", "thickness", "inward"],
    );
    let move_copy = object_schema(
        json!({
            "body_ids": body_ids.clone(),
            "translation": point3.clone(),
            "rotation": {
                "type": "array",
                "items": { "type": "number" },
                "minItems": 4,
                "maxItems": 4,
                "description": "Unit quaternion [x, y, z, w]. Default identity [0, 0, 0, 1]."
            },
            "pivot": point3.clone(),
            "copy": { "type": "boolean", "description": "When true, leave the source bodies and create copies." }
        }),
        &["body_ids", "translation", "pivot"],
    );
    let solid_mirror = object_schema(
        json!({
            "body_ids": body_ids.clone(),
            "plane": plane.clone()
        }),
        &["body_ids", "plane"],
    );
    let rectangular_pattern = object_schema(
        json!({
            "body_ids": body_ids.clone(),
            "direction": point3.clone(),
            "spacing": { "type": "number" },
            "count": { "type": "integer", "minimum": 2 },
            "second_direction": { "oneOf": [point3.clone(), {"type": "null"}] },
            "second_spacing": { "type": "number" },
            "second_count": { "type": "integer", "minimum": 1 }
        }),
        &["body_ids", "direction", "spacing", "count"],
    );
    let circular_pattern = object_schema(
        json!({
            "body_ids": body_ids.clone(),
            "axis_origin": point3.clone(),
            "axis_direction": point3,
            "count": { "type": "integer", "minimum": 2 },
            "total_angle_deg": { "type": "number", "exclusiveMinimum": -360, "maximum": 360 }
        }),
        &[
            "body_ids",
            "axis_origin",
            "axis_direction",
            "count",
            "total_angle_deg",
        ],
    );
    let combine = object_schema(
        json!({
            "target_body_id": { "type": "integer", "minimum": 1 },
            "tool_body_ids": body_ids.clone(),
            "operation": { "type": "string", "enum": ["join", "cut", "intersect"] },
            "keep_tools": { "type": "boolean" }
        }),
        &["target_body_id", "tool_body_ids", "operation", "keep_tools"],
    );
    let split_body = object_schema(
        json!({
            "body_id": { "type": "integer", "minimum": 1 },
            "plane": plane.clone()
        }),
        &["body_id", "plane"],
    );
    let import_step = object_schema(
        json!({
            "file_name": {
                "type": "string",
                "minLength": 1,
                "description": "Original STEP/STP file name stored with the import feature."
            },
            "data_base64": {
                "type": "string",
                "minLength": 1,
                "description": "Base64-encoded STEP/STP bytes. Imported as a dumb reference body, not recovered sketch/extrude history."
            }
        }),
        &["file_name", "data_base64"],
    );
    let offset_plane = object_schema(
        json!({
            "name": {"type":"string","minLength":1},
            "reference": plane.clone(),
            "distance": { "type": "number" }
        }),
        &["reference", "distance"],
    );
    let midplane = object_schema(
        json!({
            "name": {"type":"string","minLength":1},
            "first": plane.clone(),
            "second": plane.clone()
        }),
        &["first", "second"],
    );
    let plane_at_angle = object_schema(
        json!({
            "name": {"type":"string","minLength":1},
            "reference": plane,
            "body_id": { "type": "integer", "minimum": 1 },
            "edge_id": { "type": "integer", "minimum": 1 },
            "angle_deg": { "type": "number", "minimum": -360, "maximum": 360 }
        }),
        &["reference", "body_id", "edge_id", "angle_deg"],
    );

    let assembly_transform = object_schema(
        json!({
            "translation": {
                "type": "array",
                "items": {"type": "number"},
                "minItems": 3,
                "maxItems": 3
            },
            "rotation": {
                "type": "array",
                "items": {"type": "number"},
                "minItems": 4,
                "maxItems": 4,
                "description": "Unit quaternion as [x, y, z, w]."
            }
        }),
        &["translation", "rotation"],
    );
    let joint_vec3 = json!({
        "type": "array",
        "items": {"type": "number"},
        "minItems": 3,
        "maxItems": 3
    });
    let joint_frame = object_schema(
        json!({
            "origin": joint_vec3.clone(),
            "primary_axis": joint_vec3.clone(),
            "secondary_axis": joint_vec3.clone()
        }),
        &["origin", "primary_axis", "secondary_axis"],
    );
    let joint_limits = object_schema(
        json!({
            "min": {"type": "number"},
            "max": {"type": "number"}
        }),
        &["min", "max"],
    );
    let joint_connector = object_schema(
        json!({
            "body_id": {"type": "integer", "minimum": 1},
            "face_id": {"type": "integer", "minimum": 0},
            "face_key": {"type": "string"},
            "edge_id": {"type": ["integer", "null"], "minimum": 1},
            "edge_key": {"type": ["string", "null"]},
            "kind": {
                "type": "string",
                "enum": ["planar_face", "cylindrical_face", "virtual_circular_face", "circular_edge"]
            },
            "radius": {"type": ["number", "null"]},
            "source_surface_frame": object_or_null(joint_frame.clone()),
            "frame": joint_frame.clone()
        }),
        &["body_id", "face_id", "face_key", "frame"],
    );
    let joint_kind = json!({
        "type": "string",
        "enum": [
            "rigid",
            "revolute",
            "slider",
            "cylindrical",
            "planar",
            "ball",
            "pin_slot",
            "screw",
            "universal"
        ]
    });
    let joint_advanced = object_schema(
        json!({
            "secondary_angle_offset_deg": {"type": "number"},
            "tertiary_angle_offset_deg": {"type": "number"},
            "secondary_linear_offset_mm": {"type": "number"},
            "screw_pitch_mm_per_revolution": {"type": "number"},
            "connector_a_twist_deg": {"type": "number"},
            "connector_b_twist_deg": {"type": "number"},
            "secondary_angle_limits": object_or_null(joint_limits.clone()),
            "tertiary_angle_limits": object_or_null(joint_limits.clone()),
            "secondary_linear_limits": object_or_null(joint_limits.clone()),
            "connector_a_occurrence_id": {"type": ["integer", "null"], "minimum": 1},
            "connector_b_occurrence_id": {"type": ["integer", "null"], "minimum": 1}
        }),
        &[],
    );
    let mut joint_definition = object_schema(
        json!({
            "id": {"type": "integer", "minimum": 1},
            "name": {"type": "string", "minLength": 1},
            "kind": joint_kind.clone(),
            "connector_a": joint_connector.clone(),
            "connector_b": joint_connector.clone(),
            "flipped": {"type": "boolean"},
            "angle_offset_deg": {"type": "number"},
            "linear_offset_mm": {"type": "number"},
            "limits": object_or_null(joint_limits.clone()),
            "angle_limits": object_or_null(joint_limits.clone()),
            "linear_limits": object_or_null(joint_limits.clone()),
            "advanced": joint_advanced.clone(),
            "enabled": {"type": "boolean"}
        }),
        &["id", "name", "kind", "connector_a", "connector_b"],
    );

    joint_definition["description"] = json!(
        "Full replace-all JointDefinitionDto, not a patch. Required: id, name, kind, connector_a, connector_b. Omitted optional limits/frames deserialize as null and clear those values."
    );

    let mut tools = vec![
        ToolSpec::direct(
            "cad_document",
            "Inspect CAD document",
            "Return document settings, browser tree, and ordered feature history.",
            "document",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "cad_set_document_name",
            "Set document name",
            "Rename the selected Limo CAD document.",
            "document_set_name",
            Payload::Field("name"),
            object_schema(json!({"name": {"type": "string", "minLength": 1}}), &["name"]),
        ),
        ToolSpec::direct(
            "cad_project_model",
            "Export project model",
            "Return the versioned model.json payload used inside a .limo project.",
            "project_export_model",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::solid(
            "cad_load_project_model",
            "Load project model",
            "Transactionally load and recompute a Limo CAD model.json payload.",
            "project_prepare_load",
            Payload::Field("model_json"),
            object_schema(
                json!({"model_json": {"type": "string", "minLength": 2}}),
                &["model_json"],
            ),
        ),
        ToolSpec::solid(
            "cad_new_project",
            "New project",
            "Clear the headless document to a fresh empty project and recompute (resets botched sessions).",
            "project_prepare_new",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "sketch_begin",
            "Begin sketch",
            "Begin a sketch on an origin plane or stable planar FaceId, with an optional face-origin placement policy.",
            "begin_sketch",
            Payload::Object,
            object_schema(
                json!({
                    "plane": plane,
                    "name": {"type":"string","minLength":1,"description":"Unique design-intent name, retained in history, sketch references, and native save/reopen."},
                    "face_origin": {
                        "type": "string",
                        "enum": ["face_center", "global_origin_projection"],
                        "description": "For planar faces, place sketch zero at the face center or at the projected global XYZ origin."
                    }
                }),
                &["plane"],
            ),
        ),
        ToolSpec::direct(
            "sketch_finish",
            "Finish sketch",
            "Finish the active sketch and add it to feature history.",
            "end_sketch",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "sketch_edit",
            "Edit sketch",
            "Re-enter a finished sketch by name. Optional occurrence_id edits its shared definition in that placed occurrence, with surrounding parts faded. Picking and dimensions use the displayed frame; saved sources remain in definition coordinates. Finish before changing assembly placement or structure; recompute updates all shared occurrences.",
            "edit_sketch",
            Payload::Object,
            object_schema(json!({"name": {"type": "string", "minLength": 1}, "occurrence_id":{"type":"integer","minimum":1}}), &["name"]),
        ),
        ToolSpec::direct(
            "sketch_active",
            "Inspect active sketch",
            "Return the active sketch snapshot or null.",
            "active_sketch",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "sketch_finished",
            "List finished sketches",
            "Return retained snapshots of every finished sketch.",
            "finished_sketches",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "sketch_profiles",
            "List closed profiles",
            "Extract closed profile loops available to solid tools.",
            "profile_catalog",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "sketch_preview_line",
            "Preview line",
            "Resolve snapping and inferred constraints without mutating the sketch.",
            "preview_segment",
            Payload::Object,
            object_schema(
                json!({"from": point.clone(), "to_raw": point.clone(), "ctrl_held": {"type": "boolean"}}),
                &["from", "to_raw"],
            ),
        ),
        ToolSpec::direct(
            "sketch_preview_line_locked",
            "Preview locked line",
            "Preview a length/angle-locked segment without mutating the sketch (dynamic-input parity).",
            "preview_segment_locked",
            Payload::Object,
            object_schema(
                json!({
                    "from": point.clone(),
                    "to_hint": point.clone(),
                    "length_mm": {"type": "number", "exclusiveMinimum": 0},
                    "angle_deg": {"type": "number"},
                    "length_text": {"type": "string"},
                    "angle_text": {"type": "string"},
                    "ctrl_held": {"type": "boolean"}
                }),
                &["from", "to_hint"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_line",
            "Add line",
            "Add a snapped line segment to the active sketch.",
            "add_line",
            Payload::Object,
            object_schema(
                json!({"from": point.clone(), "to_raw": point.clone(), "ctrl_held": {"type": "boolean"}}),
                &["from", "to_raw"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_line_locked",
            "Add dimensioned line",
            "Add a line with optional locked length/angle values or formula text.",
            "add_line_locked",
            Payload::Object,
            dto_schema("LockedSegmentRequest: from, to_hint, optional length_mm/angle_deg or length_text/angle_text, ctrl_held."),
        ),
        ToolSpec::direct(
            "sketch_add_midpoint_line",
            "Add midpoint line",
            "Create a line symmetrically from a midpoint and endpoint.",
            "add_line_midpoint",
            Payload::Object,
            object_schema(
                json!({"mid_raw": point.clone(), "end_raw": point.clone(), "ctrl_held": {"type": "boolean"}}),
                &["mid_raw", "end_raw"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_point",
            "Add point",
            "Add a sketch point.",
            "add_point",
            Payload::Object,
            object_schema(json!({"position": point.clone()}), &["position"]),
        ),
        ToolSpec::direct(
            "sketch_add_rectangle",
            "Add rectangle",
            "Add a two-point or center rectangle.",
            "add_rectangle",
            Payload::Object,
            object_schema(
                json!({
                    "mode": {"type": "string", "enum": ["two_point", "center"]},
                    "p1": point.clone(),
                    "p2": point.clone(),
                    "ctrl_held": {"type": "boolean"}
                }),
                &["mode", "p1", "p2"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_rectangle_locked",
            "Add dimensioned rectangle",
            "Add a rectangle with optional driving width/height values or formulas.",
            "add_rectangle_locked",
            Payload::Object,
            object_schema(
                json!({
                    "mode": {"type": "string", "enum": ["two_point", "center"]},
                    "anchor": point.clone(),
                    "corner_hint": point.clone(),
                    "width_mm": {"type": ["number", "null"], "description": "Optional driving width in millimeters."},
                    "height_mm": {"type": ["number", "null"], "description": "Optional driving height in millimeters."},
                    "width_text": {"type": ["string", "null"], "description": "Optional driving width formula."},
                    "height_text": {"type": ["string", "null"], "description": "Optional driving height formula."},
                    "ctrl_held": {"type": "boolean"}
                }),
                &["mode", "anchor", "corner_hint"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_circle",
            "Add circle",
            "Add a center-diameter or two-point circle.",
            "add_circle",
            Payload::Object,
            object_schema(
                json!({
                    "mode": {"type": "string", "enum": ["center_diameter", "two_point"]},
                    "p1": point.clone(),
                    "p2": point.clone(),
                    "ctrl_held": {"type": "boolean"}
                }),
                &["mode", "p1", "p2"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_circle_locked",
            "Add dimensioned circle",
            "Add a circle with an optional driving diameter value or formula.",
            "add_circle_locked",
            Payload::Object,
            dto_schema("LockedCircleRequest: mode, anchor, edge_hint, optional diameter_mm/diameter_text, ctrl_held."),
        ),
        ToolSpec::direct(
            "sketch_add_arc_3pt",
            "Add three-point arc",
            "Add an arc through three sketch points.",
            "add_arc_3pt",
            Payload::Object,
            object_schema(
                json!({"p1": point.clone(), "p2": point.clone(), "p3": point.clone(), "ctrl_held": {"type": "boolean"}}),
                &["p1", "p2", "p3"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_arc_center",
            "Add center arc",
            "Add an arc from center, start, and sweep points.",
            "add_arc_center",
            Payload::Object,
            object_schema(
                json!({"center": point.clone(), "start": point.clone(), "sweep": point.clone(), "ctrl_held": {"type": "boolean"}}),
                &["center", "start", "sweep"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_slot",
            "Add slot",
            "Add a center-to-center, overall, or center-point slot.",
            "add_slot",
            Payload::Object,
            dto_schema("SlotRequest: mode, p1, p2, cursor, optional width_mm/width_text."),
        ),
        ToolSpec::direct(
            "sketch_add_spline",
            "Add fit-point spline",
            "Add a spline through two or more fit points.",
            "add_spline",
            Payload::Object,
            object_schema(
                json!({"points": {"type": "array", "items": point.clone(), "minItems": 2}}),
                &["points"],
            ),
        ),
        ToolSpec::direct(
            "sketch_add_constraint",
            "Add geometric constraint",
            "Add one tagged constraint such as horizontal, coincident, tangent, equal, parallel, perpendicular, fix, midpoint, concentric, collinear, or symmetry.",
            "add_constraint",
            Payload::Object,
            dto_schema("Constraint object with a snake_case `type` tag and its entity ids."),
        ),
        ToolSpec::direct(
            "sketch_add_constraints",
            "Add constraint batch",
            "Apply several tagged constraints as one transaction.",
            "add_constraints",
            Payload::Object,
            object_schema(
                json!({"constraints": {"type": "array", "items": {"type": "object"}, "minItems": 1}}),
                &["constraints"],
            ),
        ),
        ToolSpec::direct(
            "sketch_delete_constraint", "Delete geometric constraint",
            "Remove a geometric constraint by its stable constraint id.",
            "delete_constraint", Payload::Object,
            object_schema(json!({"constraint_id":{"type":"integer","minimum":1}}), &["constraint_id"]),
        ),
        ToolSpec::direct(
            "sketch_set_dimension_mode", "Set dimension mode",
            "Choose a driving dimension or a reference measurement without deleting its annotation.",
            "set_dimension_mode", Payload::Object,
            object_schema(json!({"constraint_id":{"type":"integer","minimum":1},"mode":{"type":"string","enum":["driving","reference"]}}), &["constraint_id","mode"]),
        ),
        ToolSpec::direct(
            "sketch_add_dimension",
            "Add driving dimension",
            "Add a driving dimension to selected entities, optionally using a formula.",
            "add_dimension",
            Payload::Object,
            object_schema(
                json!({
                    "entities": entity_ids.clone(),
                    "text_pos": point.clone(),
                    "value_text": {"type": ["string", "null"]}
                }),
                &["entities", "text_pos"],
            ),
        ),
        ToolSpec::direct(
            "sketch_edit_dimension",
            "Edit driving dimension",
            "Change a dimension value or formula.",
            "edit_dimension",
            Payload::Object,
            object_schema(
                json!({"constraint_id": {"type": "integer", "minimum": 1}, "text": {"type": "string"}}),
                &["constraint_id", "text"],
            ),
        ),
        ToolSpec::direct(
            "sketch_move_dimension",
            "Move dimension annotation",
            "Move a dimension's annotation position.",
            "move_dimension",
            Payload::Object,
            object_schema(
                json!({"constraint_id": {"type": "integer", "minimum": 1}, "text_pos": point.clone()}),
                &["constraint_id", "text_pos"],
            ),
        ),
        ToolSpec::direct(
            "sketch_delete_dimension",
            "Delete dimension",
            "Delete a driving dimension by constraint id.",
            "delete_dimension",
            Payload::Object,
            object_schema(
                json!({"constraint_id": {"type": "integer", "minimum": 1}}),
                &["constraint_id"],
            ),
        ),
        ToolSpec::direct(
            "sketch_fillet",
            "Fillet sketch lines",
            "Trim two intersecting lines and add a tangent arc with a driving radius.",
            "fillet_lines",
            Payload::Object,
            object_schema(
                json!({
                    "l1": {"type": "integer", "minimum": 1},
                    "l2": {"type": "integer", "minimum": 1},
                    "radius_text": {"type": "string", "minLength": 1}
                }),
                &["l1", "l2", "radius_text"],
            ),
        ),
        ToolSpec::direct(
            "sketch_chamfer",
            "Chamfer sketch lines",
            "Trim two intersecting lines and connect them with an equal-distance chamfer.",
            "chamfer_lines",
            Payload::Object,
            object_schema(
                json!({
                    "l1": {"type": "integer", "minimum": 1},
                    "l2": {"type": "integer", "minimum": 1},
                    "distance_text": {"type": "string", "minLength": 1}
                }),
                &["l1", "l2", "distance_text"],
            ),
        ),
        ToolSpec::direct(
            "sketch_offset",
            "Offset sketch curve",
            "Create an offset curve on the side selected by a cursor point.",
            "offset_curve",
            Payload::Object,
            object_schema(
                json!({
                    "entity": {"type": "integer", "minimum": 1},
                    "distance_text": {"type": "string", "minLength": 1},
                    "cursor": point.clone()
                }),
                &["entity", "distance_text", "cursor"],
            ),
        ),
        ToolSpec::direct(
            "sketch_trim",
            "Trim sketch curve",
            "Trim the clicked piece of a curve at its intersections.",
            "trim_entity",
            Payload::Object,
            object_schema(
                json!({"entity": {"type": "integer", "minimum": 1}, "click": point.clone()}),
                &["entity", "click"],
            ),
        ),
        ToolSpec::direct(
            "sketch_extend",
            "Extend sketch curve",
            "Extend the clicked end of a curve to the nearest intersection.",
            "extend_entity",
            Payload::Object,
            object_schema(
                json!({"entity": {"type": "integer", "minimum": 1}, "click": point.clone()}),
                &["entity", "click"],
            ),
        ),
        ToolSpec::direct(
            "sketch_break",
            "Break sketch curve",
            "Split a curve at a sketch-local point.",
            "break_curve",
            Payload::Object,
            object_schema(
                json!({"entity": {"type": "integer", "minimum": 1}, "at": point.clone()}),
                &["entity", "at"],
            ),
        ),
        ToolSpec::direct(
            "sketch_mirror",
            "Mirror sketch entities",
            "Mirror selected entities around an existing sketch line.",
            "mirror_entities",
            Payload::Object,
            object_schema(
                json!({"entity_ids": entity_ids.clone(), "axis_line": {"type": "integer", "minimum": 1}}),
                &["entity_ids", "axis_line"],
            ),
        ),
        ToolSpec::direct(
            "sketch_rectangular_pattern",
            "Rectangular sketch pattern",
            "Pattern selected sketch entities in one or two linear directions. Counts include the source occurrence.",
            "rectangular_pattern",
            Payload::Object,
            object_schema(
                json!({
                    "entity_ids": entity_ids.clone(),
                    "direction": point.clone(),
                    "spacing": {"type": "number"},
                    "count": {"type": "integer", "minimum": 2, "maximum": 1000},
                    "second_direction": point.clone(),
                    "second_spacing": {"type": "number"},
                    "second_count": {"type": "integer", "minimum": 1, "maximum": 1000}
                }),
                &["entity_ids", "direction", "spacing", "count"],
            ),
        ),
        ToolSpec::direct(
            "sketch_circular_pattern",
            "Circular sketch pattern",
            "Pattern selected sketch entities around a sketch-local center. Count includes the source occurrence.",
            "circular_pattern",
            Payload::Object,
            object_schema(
                json!({
                    "entity_ids": entity_ids.clone(),
                    "center": point.clone(),
                    "count": {"type": "integer", "minimum": 2, "maximum": 1000},
                    "total_angle_deg": {"type": "number"}
                }),
                &["entity_ids", "center", "count", "total_angle_deg"],
            ),
        ),
        ToolSpec::direct(
            "sketch_move_copy",
            "Move or copy sketch entities",
            "Translate selected entities, either in place or as copies.",
            "move_copy_entities",
            Payload::Object,
            object_schema(
                json!({
                    "entity_ids": entity_ids.clone(),
                    "dx": {"type": "number"},
                    "dy": {"type": "number"},
                    "copy": {"type": "boolean"}
                }),
                &["entity_ids", "dx", "dy", "copy"],
            ),
        ),
        ToolSpec::direct(
            "sketch_scale",
            "Scale sketch entities",
            "Scale selected entities around a sketch-local origin.",
            "scale_entities",
            Payload::Object,
            object_schema(
                json!({
                    "entity_ids": entity_ids.clone(),
                    "origin": point.clone(),
                    "factor_text": {"type": "string", "minLength": 1}
                }),
                &["entity_ids", "origin", "factor_text"],
            ),
        ),
        ToolSpec::direct(
            "sketch_polygon",
            "Create sketch polygon",
            "Create an inscribed or circumscribed regular polygon.",
            "polygon_create",
            Payload::Object,
            object_schema(
                json!({
                    "center": point.clone(),
                    "edge_count": {"type": "integer", "minimum": 3},
                    "radius_text": {"type": "string", "minLength": 1},
                    "rotation_deg": {"type": "number"},
                    "mode": {"type": "string", "enum": ["inscribed", "circumscribed"]}
                }),
                &["center", "edge_count", "radius_text", "rotation_deg", "mode"],
            ),
        ),
        ToolSpec::direct(
            "sketch_move_point",
            "Move sketch point",
            "Move a point through the solver; use phase=single for one scripted operation.",
            "move_point",
            Payload::Object,
            object_schema(
                json!({
                    "point_id": {"type": "integer", "minimum": 1},
                    "to_raw": point.clone(),
                    "ctrl_held": {"type": "boolean"},
                    "phase": {"type": "string", "enum": ["begin", "update", "end", "single"]}
                }),
                &["point_id", "to_raw"],
            ),
        ),
        ToolSpec::direct(
            "sketch_toggle_fix",
            "Fix or unfix entities",
            "Toggle Fix on a batch of sketch entities.",
            "toggle_fix_entities",
            Payload::Object,
            object_schema(json!({"entity_ids": entity_ids.clone()}), &["entity_ids"]),
        ),
        ToolSpec::direct(
            "sketch_delete_entities",
            "Delete sketch entities",
            "Delete one or more sketch entities as one undoable operation.",
            "delete_entities",
            Payload::Object,
            object_schema(json!({"entity_ids": entity_ids}), &["entity_ids"]),
        ),
        ToolSpec::direct(
            "sketch_undo",
            "Undo sketch command",
            "Undo the active sketch's last command.",
            "undo",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "sketch_redo",
            "Redo sketch command",
            "Redo the active sketch's next command.",
            "redo",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "sketch_set_grid_snap",
            "Set sketch grid snapping",
            "Enable or disable grid snapping for the active and future sketches.",
            "set_grid_snap",
            Payload::Object,
            object_schema(json!({"enabled": {"type": "boolean"}}), &["enabled"]),
        ),
        ToolSpec::direct(
            "sketch_set_grid_step",
            "Set sketch grid step",
            "Set the sketch grid step size in millimetres (matches UI grid precision).",
            "set_grid_step",
            Payload::Object,
            object_schema(
                json!({"step_mm": {"type": "number", "exclusiveMinimum": 0}}),
                &["step_mm"],
            ),
        ),
        ToolSpec::direct(
            "sketch_eval_expression",
            "Evaluate sketch expression",
            "Evaluate a number or parameter formula in the active sketch.",
            "eval_expression",
            Payload::Object,
            object_schema(json!({"text": {"type": "string", "minLength": 1}}), &["text"]),
        ),
        ToolSpec::direct(
            "sketch_set_dimension_style",
            "Set dimension style",
            "Use aligned or ISO 129 sketch dimension annotations.",
            "set_dimension_style",
            Payload::Object,
            object_schema(
                json!({"style": {"type": "string", "enum": ["aligned", "iso"]}}),
                &["style"],
            ),
        ),
        ToolSpec::direct(
            "sketch_preview_fillet",
            "Preview sketch fillet",
            "Return the tangent arc and trim points for two lines without mutating the sketch.",
            "fillet_preview",
            Payload::Object,
            object_schema(
                json!({
                    "l1": {"type": "integer", "minimum": 1},
                    "l2": {"type": "integer", "minimum": 1},
                    "radius_text": {"type": "string", "minLength": 1}
                }),
                &["l1", "l2", "radius_text"],
            ),
        ),
        ToolSpec::direct(
            "sketch_preview_offset",
            "Preview sketch offset",
            "Return an offset curve without mutating the sketch.",
            "offset_preview",
            Payload::Object,
            object_schema(
                json!({
                    "entity": {"type": "integer", "minimum": 1},
                    "distance_text": {"type": "string", "minLength": 1},
                    "cursor": point.clone()
                }),
                &["entity", "distance_text", "cursor"],
            ),
        ),
        ToolSpec::direct(
            "sketch_preview_trim",
            "Preview sketch trim",
            "Return kept and removed curve pieces without mutating the sketch.",
            "trim_preview",
            Payload::Object,
            object_schema(
                json!({"entity": {"type": "integer", "minimum": 1}, "click": point}),
                &["entity", "click"],
            ),
        ),
        ToolSpec::direct(
            "construction_plane_definitions",
            "List construction planes",
            "Return persisted offset, midplane, and plane-at-angle definitions with stable datum IDs and resolved bases.",
            "datum_plane_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "construction_set_visibility",
            "Show or hide retained sketches and datums",
            "Change saved construction-reference visibility without changing geometry or body visibility. Omit both selectors to affect all retained sketches and datum planes; provide either selector to affect only the explicit sets. Unknown references reject atomically. The active unfinished sketch stays visible; newly created references remain visible.",
            "construction_set_visibility",
            Payload::Object,
            object_schema(json!({
                "visible":{"type":"boolean"},
                "sketch_names":{"type":"array","items":{"type":"string","minLength":1}},
                "datum_plane_ids":{"type":"array","items":{"type":"integer","minimum":1}}
            }), &["visible"]),
        ),
        ToolSpec::direct(
            "project_visibility",
            "Read saved model visibility",
            "Return the Browser's saved hidden body IDs, datum plane IDs, and retained sketch names. Geometry and exports are unaffected by visibility.",
            "project_visibility",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "project_set_visibility",
            "Set saved model visibility",
            "Replace the Browser's complete saved visibility snapshot. Read project_visibility first to preserve other choices. All three arrays are required; empty arrays show everything. Like the app, this normalizes duplicates and removes stale references. Assembly mesh exports honor body visibility; definition exports retain selected body definitions regardless of visibility.",
            "project_set_visibility",
            Payload::Object,
            object_schema(json!({
                "hidden_body_ids":{"type":"array","items":{"type":"integer","minimum":1}},
                "hidden_datum_plane_ids":{"type":"array","items":{"type":"integer","minimum":1}},
                "hidden_sketch_names":{"type":"array","items":{"type":"string","minLength":1}}
            }), &["hidden_body_ids", "hidden_datum_plane_ids", "hidden_sketch_names"]),
        ),
        ToolSpec::direct(
            "named_views",
            "Read named view configurations",
            "Return saved review views and the view recalled in this session, if any. Each view stores a name, camera, visible body ids, and optional display offsets. Geometry is unchanged.",
            "named_views",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "named_view_solution",
            "Resolve named view placement",
            "Return the same occurrence solution used by export: absent name uses current presentation and live visibility; empty name uses assembled placement and live visibility; nonempty name uses its saved snapshot. This read changes no geometry or layout.",
            "named_view_solution",
            Payload::Object,
            object_schema(json!({"name":{"type":["string","null"],"maxLength":200}}), &[]),
        ),
        ToolSpec::direct(
            "set_named_views",
            "Replace named view configurations",
            "Replace saved presentation and print views. Prefer upsert_named_view to change one view. occurrence_offsets move and rotate occurrences and descendants without editing mechanical placement. Optional print_layout and print_bed enable checks; legacy part_offsets remain readable. Unknown IDs or stale expected_model_json reject the whole list. Metadata edits clear the active view.",
            "set_named_views",
            Payload::Object,
            object_schema(json!({"expected_model_json":{"type":"string"},"views":{"type":"array","items":named_view_schema()}}), &["views"]),
        ),
        ToolSpec::direct(
            "upsert_named_view",
            "Save or update one named view",
            "Save one presentation or print view without replacing other views. In the live Bevy app, use the Named Views controls or cad_interface inspect view_state to capture the current camera, visible body IDs and display offsets. Supply explicit camera coordinates and visible body IDs in headless mode. Occurrence offsets retain intentional repeats and do not edit mechanical geometry; metadata edits clear the active view.",
            "upsert_named_view",
            Payload::Object,
            named_view_schema(),
        ),
        ToolSpec::direct(
            "rename_named_view",
            "Rename one named view",
            "Rename a saved view while preserving its camera, visibility and offsets. Unknown names and duplicate new names reject atomically. Metadata edits clear the active view.",
            "rename_named_view",
            Payload::Object,
            object_schema(json!({"name":{"type":"string","minLength":1,"maxLength":200},"new_name":{"type":"string","minLength":1,"maxLength":200}}), &["name", "new_name"]),
        ),
        ToolSpec::direct(
            "delete_named_view",
            "Delete one named view",
            "Delete a saved review view without changing geometry, visibility or other saved views. Unknown names reject atomically. Metadata edits clear the active view.",
            "delete_named_view",
            Payload::Object,
            object_schema(json!({"name":{"type":"string","minLength":1,"maxLength":200}}), &["name"]),
        ),
        ToolSpec::direct(
            "recall_named_view",
            "Recall a named view",
            "Show only the view's visible bodies and return its camera and display offsets. Part offsets are not written into solid geometry. Unknown names reject without changing visibility.",
            "recall_named_view",
            Payload::Object,
            object_schema(json!({"name":{"type":"string","minLength":1,"maxLength":200}}), &["name"]),
        ),
        ToolSpec::direct(
            "clear_named_view",
            "Return to assembled view",
            "Clear the recalled view's display offsets and active marker without editing saved views, visibility, or geometry.",
            "clear_named_view",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "construction_plane_offset",
            "Create offset construction plane",
            "Create a construction plane at a signed distance from an origin plane, planar face, or existing datum plane.",
            "datum_plane_create",
            Payload::DatumSource("offset"),
            offset_plane.clone(),
        ),
        ToolSpec::direct(
            "construction_plane_edit_offset",
            "Edit offset construction plane",
            "Edit an offset-plane feature while preserving its feature and datum IDs.",
            "datum_plane_edit",
            Payload::EditDatumSource("offset"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "reference": offset_plane["properties"]["reference"].clone(),
                    "distance": {"type": "number"}
                }),
                &["feature_id", "reference", "distance"],
            ),
        ),
        ToolSpec::direct(
            "construction_plane_midplane",
            "Create midplane",
            "Create a construction plane halfway between two parallel plane references.",
            "datum_plane_create",
            Payload::DatumSource("midplane"),
            midplane.clone(),
        ),
        ToolSpec::direct(
            "construction_plane_edit_midplane",
            "Edit midplane",
            "Edit a midplane feature while preserving its feature and datum IDs.",
            "datum_plane_edit",
            Payload::EditDatumSource("midplane"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "first": midplane["properties"]["first"].clone(),
                    "second": midplane["properties"]["second"].clone()
                }),
                &["feature_id", "first", "second"],
            ),
        ),
        ToolSpec::direct(
            "construction_plane_at_angle",
            "Create plane at angle",
            "Rotate a reference plane around a stable straight body edge lying on that plane.",
            "datum_plane_create",
            Payload::DatumSource("at_angle"),
            plane_at_angle.clone(),
        ),
        ToolSpec::direct(
            "construction_plane_edit_at_angle",
            "Edit plane at angle",
            "Edit a plane-at-angle feature while preserving its feature and datum IDs.",
            "datum_plane_edit",
            Payload::EditDatumSource("at_angle"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "reference": plane_at_angle["properties"]["reference"].clone(),
                    "body_id": {"type": "integer", "minimum": 1},
                    "edge_id": {"type": "integer", "minimum": 1},
                    "angle_deg": {"type": "number", "minimum": -360, "maximum": 360}
                }),
                &["feature_id", "reference", "body_id", "edge_id", "angle_deg"],
            ),
        ),
        ToolSpec::direct(
            "solid_scene",
            "Inspect solid scene",
            "Return active bodies, stable Body/Face/Edge ids, meshes, and feature errors.",
            "solid_scene",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_tessellate",
            "Tessellate bodies",
            "Tessellate active bodies with configurable deflection and return mesh stats (no file bytes). Use before export to judge triangle density.",
            "solid_tessellate",
            Payload::Object,
            object_schema(
                json!({
                    "body_ids": {
                        "type": "array",
                        "items": {"type": "integer", "minimum": 1}
                    },
                    "linear_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.15},
                    "angular_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.35}
                }),
                &[],
            ),
        ),
        ToolSpec::direct(
            "solid_extrude_definitions",
            "List Extrude definitions",
            "Return persisted Extrude feature parameters.",
            "extrude_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_revolve_definitions",
            "List Revolve definitions",
            "Return persisted Revolve feature parameters.",
            "revolve_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_sweep_definitions",
            "List Sweep definitions",
            "Return persisted Sweep profile and path references.",
            "sweep_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_loft_definitions",
            "List Loft definitions",
            "Return persisted ordered Loft profile sections.",
            "loft_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_rib_definitions",
            "List Rib definitions",
            "Return persisted Rib centerline, thickness, and depth parameters.",
            "rib_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_fillet_definitions",
            "List solid Fillet definitions",
            "Return persisted solid-edge Fillet parameters and stable edge references.",
            "fillet_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_chamfer_definitions",
            "List solid Chamfer definitions",
            "Return persisted solid-edge Chamfer parameters and stable edge references.",
            "chamfer_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_hole_definitions",
            "List Hole definitions",
            "Return persisted planar-face Hole parameters and stable face references.",
            "hole_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "solid_body_feature_definitions",
            "List body-operation definitions",
            "Return persisted Shell, Mirror, Pattern, Combine, and Split Body definitions.",
            "body_feature_definitions",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::solid(
            "solid_extrude",
            "Extrude sketch profiles",
            "Create or boolean Extrude selected closed profiles and fully replay feature history.",
            "solid_prepare_extrude",
            Payload::Object,
            extrude.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_extrude",
            "Edit Extrude feature",
            "Edit one persisted Extrude feature and fully replay downstream history.",
            "solid_prepare_edit_extrude",
            Payload::Object,
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "extrude": extrude
                }),
                &["feature_id", "extrude"],
            ),
        ),
        ToolSpec::solid(
            "solid_revolve",
            "Revolve sketch profiles",
            "Create or boolean solids by revolving selected profiles around a manual or stable sketch-line axis.",
            "solid_prepare_revolve",
            Payload::Object,
            revolve.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_revolve",
            "Edit Revolve feature",
            "Edit one persisted Revolve feature and fully replay downstream history.",
            "solid_prepare_edit_revolve",
            Payload::Object,
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "revolve": revolve
                }),
                &["feature_id", "revolve"],
            ),
        ),
        ToolSpec::solid(
            "solid_sweep",
            "Sweep a sketch profile",
            "Sweep one closed profile along an ordered connected line, arc, circle, or spline path, with orientation, corner-transition, C1, and guide-rail controls.",
            "solid_prepare_sweep",
            Payload::Object,
            sweep.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_sweep",
            "Edit Sweep feature",
            "Edit a persisted Sweep and fully replay downstream history.",
            "solid_prepare_edit_sweep",
            Payload::Object,
            object_schema(json!({"feature_id": {"type": "integer", "minimum": 1}, "sweep": sweep}), &["feature_id", "sweep"]),
        ),
        ToolSpec::solid(
            "solid_loft",
            "Loft sketch profiles",
            "Create a solid through two or more ordered closed profile sections with G0/G1/G2 continuity and optional centerline or guide rail.",
            "solid_prepare_loft",
            Payload::Object,
            loft.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_loft",
            "Edit Loft feature",
            "Edit a persisted Loft and fully replay downstream history.",
            "solid_prepare_edit_loft",
            Payload::Object,
            object_schema(json!({"feature_id": {"type": "integer", "minimum": 1}, "loft": loft}), &["feature_id", "loft"]),
        ),
        ToolSpec::solid(
            "solid_rib",
            "Create Rib from sketch curves",
            "Create thin solids from stable line, arc, circle, or spline centerlines using Distance, To Next, Up to Face, or Through All extents.",
            "solid_prepare_rib",
            Payload::Object,
            rib.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_rib",
            "Edit Rib feature",
            "Edit a persisted Rib and fully replay downstream history.",
            "solid_prepare_edit_rib",
            Payload::Object,
            object_schema(json!({"feature_id": {"type": "integer", "minimum": 1}, "rib": rib}), &["feature_id", "rib"]),
        ),
        ToolSpec::solid(
            "solid_fillet",
            "Fillet solid edges",
            "Round one or more stable solid edges and replay downstream feature history.",
            "solid_prepare_fillet",
            Payload::Object,
            solid_fillet.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_fillet",
            "Edit solid Fillet feature",
            "Edit a persisted solid Fillet and fully replay downstream history.",
            "solid_prepare_edit_fillet",
            Payload::Object,
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "fillet": solid_fillet
                }),
                &["feature_id", "fillet"],
            ),
        ),
        ToolSpec::solid(
            "solid_chamfer",
            "Chamfer solid edges",
            "Bevel one or more stable solid edges and replay downstream feature history.",
            "solid_prepare_chamfer",
            Payload::Object,
            solid_chamfer.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_chamfer",
            "Edit solid Chamfer feature",
            "Edit a persisted solid Chamfer and fully replay downstream history.",
            "solid_prepare_edit_chamfer",
            Payload::Object,
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "chamfer": solid_chamfer
                }),
                &["feature_id", "chamfer"],
            ),
        ),
        ToolSpec::solid(
            "solid_hole",
            "Create Hole on planar face",
            "Cut one or more simple, counterbored, countersunk, or ISO/Unified threaded holes with flat or angled drill-point bottoms from a stable planar face.",
            "solid_prepare_hole",
            Payload::Object,
            hole.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_hole",
            "Edit Hole feature",
            "Edit a persisted Hole and fully replay downstream history.",
            "solid_prepare_edit_hole",
            Payload::Object,
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "hole": hole
                }),
                &["feature_id", "hole"],
            ),
        ),
        ToolSpec::solid(
            "solid_external_thread",
            "Create external thread on a cylindrical face",
            "Cut a persisted ISO metric or Unified male thread into an exact cylindrical face. The nominal diameter must match the selected cylinder. Modeled representation creates a real helix; a later planar cut can create an interrupted D section.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("external_thread"),
            external_thread.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_external_thread",
            "Edit external Thread feature",
            "Edit the persisted external thread and recompute all downstream features using its captured cylindrical reference.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("external_thread"),
            object_schema(json!({"feature_id":{"type":"integer","minimum":1},"request":external_thread}), &["feature_id", "request"]),
        ),
        ToolSpec::solid(
            "solid_shell",
            "Shell body",
            "Remove selected stable faces and offset the remaining body walls to create a hollow solid.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("shell"),
            shell.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_shell",
            "Edit Shell feature",
            "Edit a persisted Shell and fully replay downstream history.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("shell"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "request": shell
                }),
                &["feature_id", "request"],
            ),
        ),
        ToolSpec::solid(
            "solid_move_copy",
            "Move or copy bodies",
            "Apply a rigid transform to one or more bodies. Rotation is a unit quaternion [x, y, z, w]; translation and pivot are millimetres. copy=true leaves the sources and creates new bodies.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("move_copy"),
            move_copy.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_move_copy",
            "Edit Move/Copy feature",
            "Edit a persisted Move/Copy and fully replay downstream history.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("move_copy"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "request": move_copy
                }),
                &["feature_id", "request"],
            ),
        ),
        ToolSpec::solid(
            "solid_mirror",
            "Mirror bodies",
            "Create mirrored copies of one or more bodies around an origin, face, or construction plane.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("mirror"),
            solid_mirror.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_mirror",
            "Edit Mirror feature",
            "Edit a persisted body Mirror and fully replay downstream history.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("mirror"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "request": solid_mirror
                }),
                &["feature_id", "request"],
            ),
        ),
        ToolSpec::solid(
            "solid_rectangular_pattern",
            "Rectangular body pattern",
            "Copy bodies along one or two linear directions with stable pattern history.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("rectangular_pattern"),
            rectangular_pattern.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_rectangular_pattern",
            "Edit rectangular body pattern",
            "Edit a persisted Rectangular Pattern and fully replay downstream history.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("rectangular_pattern"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "request": rectangular_pattern
                }),
                &["feature_id", "request"],
            ),
        ),
        ToolSpec::solid(
            "solid_circular_pattern",
            "Circular body pattern",
            "Copy bodies around a world-space axis through a partial or full angle.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("circular_pattern"),
            circular_pattern.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_circular_pattern",
            "Edit circular body pattern",
            "Edit a persisted Circular Pattern and fully replay downstream history.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("circular_pattern"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "request": circular_pattern
                }),
                &["feature_id", "request"],
            ),
        ),
        ToolSpec::solid(
            "solid_combine",
            "Combine bodies",
            "Join, cut, or intersect a target body with one or more tool bodies.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("combine"),
            combine.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_combine",
            "Edit Combine feature",
            "Edit a persisted Combine and fully replay downstream history.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("combine"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "request": combine
                }),
                &["feature_id", "request"],
            ),
        ),
        ToolSpec::solid(
            "solid_split_body",
            "Split body",
            "Split a body into two stable bodies using an origin, planar-face, or construction plane.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("split_body"),
            split_body.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_split_body",
            "Edit Split Body feature",
            "Edit a persisted Split Body and fully replay downstream history.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("split_body"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "request": split_body
                }),
                &["feature_id", "request"],
            ),
        ),
        ToolSpec::solid(
            "solid_import_step",
            "Import STEP body",
            "Import a licensed STEP/STP file as a persistent reference solid. The kernel stores the source bytes and tessellates a dumb body; this does not recover sketch/extrude feature history.",
            "solid_prepare_body_feature",
            Payload::BodyFeature("import_step"),
            import_step.clone(),
        ),
        ToolSpec::solid(
            "solid_edit_import_step",
            "Edit STEP import feature",
            "Replace the stored STEP source on a persisted Import feature and fully replay downstream history. Still a reference solid, not reverse-engineered feature history.",
            "solid_prepare_edit_body_feature",
            Payload::EditBodyFeature("import_step"),
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "request": import_step
                }),
                &["feature_id", "request"],
            ),
        ),
        ToolSpec::solid(
            "solid_recompute",
            "Recompute solids",
            "Fully replay active solid feature history through native OCCT.",
            "solid_prepare_recompute",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::solid(
            "solid_set_rollback",
            "Move rollback marker",
            "Set the active feature count and recompute the resulting bodies.",
            "solid_prepare_set_rollback",
            Payload::Object,
            object_schema(
                json!({"rollback_index": {"type": "integer", "minimum": 0}}),
                &["rollback_index"],
            ),
        ),
        ToolSpec::direct(
            "solid_rename_feature",
            "Rename history operation",
            "Set a descriptive name on a sketch or solid history feature without recomputing geometry. Sketch renames rebind persisted references while preserving operation IDs. Names persist through edits, undo/redo and save/reopen. Name construction planes when creating them.",
            "solid_rename_feature",
            Payload::Object,
            object_schema(json!({"feature_id":{"type":"integer","minimum":1},"name":{"type":"string","minLength":1,"maxLength":256}}), &["feature_id","name"]),
        ),
        ToolSpec::solid(
            "solid_delete_feature",
            "Delete history feature",
            "Delete one history feature and recompute later features, preserving explicit broken-reference errors.",
            "solid_prepare_delete_feature",
            Payload::Object,
            object_schema(
                json!({"feature_id": {"type": "integer", "minimum": 1}}),
                &["feature_id"],
            ),
        ),
        ToolSpec::solid(
            "solid_reorder_feature",
            "Reorder history feature",
            "Move a feature to a timeline insertion slot and recompute. Dependency-breaking moves are rejected.",
            "solid_prepare_reorder_feature",
            Payload::Object,
            object_schema(
                json!({
                    "feature_id": {"type": "integer", "minimum": 1},
                    "target_index": {"type": "integer", "minimum": 0}
                }),
                &["feature_id", "target_index"],
            ),
        ),
        ToolSpec::direct(
            "assembly_document",
            "Inspect assembly document",
            "Return the host-neutral assembly document: component definitions, occurrences, joints, and grounding.",
            "assembly_document",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "assembly_solution",
            "Inspect assembly solution",
            "Return the current host-neutral assembly forward-kinematics solution (occurrence and body poses).",
            "assembly_solution",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "assembly_interference_check", "Inspect exact assembly interference",
            "Check retained native solids at solved occurrence poses. Reports exact overlap volumes and clearances after broad-phase culling. Empty occurrence_ids checks all visible occurrences; touching faces are not volumetric interference.",
            "assembly_interference_check", Payload::Object,
            object_schema(json!({"occurrence_ids":{"type":"array","items":{"type":"integer","minimum":1},"uniqueItems":true},"clearance_threshold_mm":{"type":"number","minimum":0}}), &[]),
        ),
        ToolSpec::direct(
            "assembly_create_component",
            "Create assembly component",
            "Create a reusable component definition from body ids. Use absorb_promoted_bodies to replace auto-promoted one-body components.",
            "assembly_create_component",
            Payload::Object,
            object_schema(
                json!({
                    "name": {"type": "string", "minLength": 1},
                    "body_ids": {
                        "type": "array",
                        "items": {"type": "integer", "minimum": 1},
                        "uniqueItems": true
                    },
                    "local_coordinate_system": assembly_transform.clone(),
                    "absorb_promoted_bodies": {
                        "type": "boolean",
                        "description": "Replace automatically promoted one-body root occurrences for these bodies."
                    }
                }),
                &["name"],
            ),
        ),
        ToolSpec::direct(
            "assembly_update_component",
            "Update assembly component",
            "Patch a component definition. Only id is required; omitted name/body_ids/local_coordinate_system/promoted keep their current values (ComponentDefinitionPatchDto).",
            "assembly_update_component",
            Payload::Object,
            object_schema(
                json!({
                    "component": object_schema(
                        json!({
                            "id": {"type": "integer", "minimum": 1},
                            "name": {"type": "string", "minLength": 1},
                            "body_ids": {
                                "type": "array",
                                "items": {"type": "integer", "minimum": 1},
                                "uniqueItems": true
                            },
                            "local_coordinate_system": assembly_transform.clone(),
                            "promoted": {"type": "boolean"}
                        }),
                        &["id"],
                    )
                }),
                &["component"],
            ),
        ),
        ToolSpec::direct(
            "assembly_create_occurrence",
            "Create assembly occurrence",
            "Instantiate a component definition as a new occurrence with an optional parent and local pose.",
            "assembly_create_occurrence",
            Payload::Object,
            object_schema(
                json!({
                    "component_id": {"type": "integer", "minimum": 1},
                    "name": {"type": "string", "minLength": 1},
                    "parent_occurrence_id": {"type": ["integer", "null"], "minimum": 1},
                    "local_pose": assembly_transform.clone()
                }),
                &["component_id", "name"],
            ),
        ),
        ToolSpec::direct(
            "assembly_duplicate_occurrence", "Duplicate component instance",
            "Copy a component instance and its complete subtree, preserving reusable definitions and internal joints. Parent and local pose are optional.",
            "assembly_duplicate_occurrence", Payload::Object,
            object_schema(json!({"occurrence_id":{"type":"integer","minimum":1},"parent_occurrence_id":{"type":["integer","null"],"minimum":1},"local_pose":assembly_transform.clone()}), &["occurrence_id"]),
        ),
        ToolSpec::direct(
            "assembly_remove_occurrence",
            "Remove component instance",
            "Remove one unreferenced leaf occurrence while retaining its reusable definition and source geometry. Release grounding first; children, joints, contact sets, saved-view offsets, drawing references and print bindings must be changed explicitly. The last instance of a definition must be retained or hidden.",
            "assembly_remove_occurrence",
            Payload::Object,
            object_schema(
                json!({"occurrence_id": {"type": "integer", "minimum": 1}}),
                &["occurrence_id"],
            ),
        ),
        ToolSpec::direct(
            "assembly_update_occurrence",
            "Update assembly occurrence",
            "Patch an occurrence record. Only id is required; omitted name/component_id/parent/pose/visibility/grounded keep their current values (ComponentOccurrencePatchDto).",
            "assembly_update_occurrence",
            Payload::Object,
            object_schema(
                json!({
                    "occurrence": object_schema(
                        json!({
                            "id": {"type": "integer", "minimum": 1},
                            "name": {"type": "string", "minLength": 1},
                            "component_id": {"type": "integer", "minimum": 1},
                            "parent_occurrence_id": {"type": ["integer", "null"], "minimum": 1},
                            "local_pose": assembly_transform.clone(),
                            "visible": {"type": "boolean"},
                            "grounded": {"type": "boolean"}
                        }),
                        &["id"],
                    )
                }),
                &["occurrence"],
            ),
        ),
        ToolSpec::direct(
            "assembly_set_occurrence_pose",
            "Set occurrence pose",
            "Set the parent-local pose of an occurrence (translation + x/y/z/w quaternion).",
            "assembly_set_occurrence_pose",
            Payload::Object,
            object_schema(
                json!({
                    "occurrence_id": {"type": "integer", "minimum": 1},
                    "local_pose": assembly_transform.clone()
                }),
                &["occurrence_id", "local_pose"],
            ),
        ),
        ToolSpec::direct(
            "assembly_set_occurrence_grounded",
            "Set occurrence grounded",
            "Ground or unground an occurrence. Only one occurrence may be grounded within each sibling group.",
            "assembly_set_occurrence_grounded",
            Payload::Object,
            object_schema(
                json!({
                    "occurrence_id": {"type": "integer", "minimum": 1},
                    "grounded": {"type": "boolean"}
                }),
                &["occurrence_id", "grounded"],
            ),
        ),
        ToolSpec::direct(
            "assembly_create_joint",
            "Create assembly joint",
            "Create a host-neutral joint from two topology-backed connectors (CreateJointRequestDto). Query the result with assembly_document.",
            "assembly_create_joint",
            Payload::Object,
            object_schema(
                json!({
                    "name": {"type": "string", "minLength": 1},
                    "kind": joint_kind.clone(),
                    "connector_a": joint_connector.clone(),
                    "connector_b": joint_connector.clone(),
                    "flipped": {"type": "boolean"},
                    "angle_offset_deg": {"type": "number"},
                    "linear_offset_mm": {"type": "number"},
                    "limits": object_or_null(joint_limits.clone()),
                    "angle_limits": object_or_null(joint_limits.clone()),
                    "linear_limits": object_or_null(joint_limits.clone()),
                    "advanced": joint_advanced.clone(),
                    "grounded_body_id": {"type": ["integer", "null"], "minimum": 1},
                    "grounded_occurrence_id": {"type": ["integer", "null"], "minimum": 1}
                }),
                &["name", "kind", "connector_a", "connector_b"],
            ),
        ),
        ToolSpec::direct(
            "assembly_update_joint",
            "Update assembly joint",
            "Replace-all UpdateJointRequestDto â€” not a patch. Send the full queried joint (required: id, name, kind, connector_a, connector_b). Id+name-only is schema-invalid. Omitted or JSON-null optional fields (limits, angle_limits, linear_limits, source_surface_frame) both clear those values. Re-canonicalizes connectors against live topology.",
            "assembly_update_joint",
            Payload::Object,
            object_schema(
                json!({
                    "joint": joint_definition.clone(),
                    "grounded_body_id": {"type": ["integer", "null"], "minimum": 1},
                    "grounded_occurrence_id": {"type": ["integer", "null"], "minimum": 1}
                }),
                &["joint"],
            ),
        ),
        ToolSpec::direct(
            "assembly_delete_joint", "Delete assembly joint",
            "Delete the joint by stable id through the normal engine operation. The same call applies through the live engine when attached.",
            "assembly_delete_joint", Payload::Field("joint_id"),
            object_schema(json!({"joint_id":{"type":"integer","minimum":1}}), &["joint_id"]),
        ),
        ToolSpec::direct(
            "assembly_set_joint_enabled", "Enable or suppress assembly joint",
            "Change only the joint enabled flag, preserving connectors, limits and occurrence bindings.",
            "assembly_set_joint_enabled", Payload::Object,
            object_schema(json!({"joint_id":{"type":"integer","minimum":1},"enabled":{"type":"boolean"}}), &["joint_id","enabled"]),
        ),
        ToolSpec::direct(
            "assembly_set_joint_motion", "Set joint primary motion",
            "Drive the selected primary coordinates while solving passive joints and persisted gear relations. Limits or unreachable closure reject atomically. Angles are unwrapped degrees; travel is millimetres. Query assembly_document for solved coordinates and assembly_solution for poses.",
            "assembly_set_joint_motion", Payload::Object,
            object_schema(json!({"joint_id":{"type":"integer","minimum":1},"angle_offset_deg":{"type":"number"},"linear_offset_mm":{"type":"number"}}), &["joint_id","angle_offset_deg","linear_offset_mm"]),
        ),
        ToolSpec::direct(
            "assembly_create_gear_relation", "Create geared rotation relation",
            "Persist a relation between two revolute joints: angle_b = phase_deg + sign * teeth_a/teeth_b * angle_a. reverse=true gives opposite rotation. Driving either joint solves the other; this is an ideal kinematic relation, not a contact or strength analysis.",
            "assembly_create_gear_relation",Payload::Object,gear_relation_schema(false),
        ),
        ToolSpec::direct(
            "assembly_update_gear_relation", "Update geared rotation relation",
            "Replace a saved gear relation, retaining its ID. Recompute from joint_a and reject invalid limits or closure without changing the document.",
            "assembly_update_gear_relation",Payload::Object,gear_relation_schema(true),
        ),
        ToolSpec::direct(
            "assembly_delete_gear_relation", "Delete geared rotation relation",
            "Delete only the named relation; preserve both joints and their current angles.",
            "assembly_delete_gear_relation",Payload::Field("relation_id"),object_schema(json!({"relation_id":{"type":"integer","minimum":1}}), &["relation_id"]),
        ),
        ToolSpec::direct(
            "solid_export_step",
            "Export STEP",
            "Export selected or all active bodies as AP242 STEP bytes encoded in base64. Prefer solid_export_3mf for slicers.",
            "solid_export_step",
            Payload::Object,
            object_schema(
                json!({
                    "expected_model_json": {"type":"string","description":"Optional exact cad_project_model string. Rejects if the current model differs before exporting geometry."},
                    "occurrences": {"type":"array","description":"Optional solved occurrence copies; omit for part-local geometry.","items":{
                        "type":"object","additionalProperties":false,
                        "required":["occurrence_id","component_id","body_id","translation","rotation"],
                        "properties":{
                            "occurrence_id":{"type":"integer","minimum":1},
                            "component_id":{"type":"integer","minimum":1},
                            "body_id":{"type":"integer","minimum":1},
                            "name":{"type":"string"},
                            "translation":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},
                            "rotation":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4}
                        }
                    }},
                    "body_ids": {
                        "type": "array",
                        "items": {"type": "integer", "minimum": 1},
                        "uniqueItems": true
                    },
                    "thread_metadata": {
                        "type": "array",
                        "items": {"type": "object", "additionalProperties": true}
                    }
                }),
                &[],
            ),
        ),
        ToolSpec::direct(
            "solid_export_stl",
            "Export STL",
            "Tessellate active bodies and return binary STL (millimetres) as base64. Appearance is not included.",
            "solid_export_stl",
            Payload::Object,
            object_schema(
                json!({
                    "body_ids": {
                        "type": "array",
                        "items": {"type": "integer", "minimum": 1},
                        "description": "Empty exports every active body."
                    },
                    "named_view":{"type":"string","description":"Saved layout name; requires assembly scope. Includes every visible repetition."},
                    "print_bed":print_bed_schema(),
                    "scope": {"type":"string","enum":["assembly","definition"],"default":"assembly","description":"Assembly exports visible solved occurrences. Definition exports each selected body once in its part coordinates."},
                    "expected_model_json": {"type":"string","description":"Optional exact cad_project_model string captured before an interactive choice. Export rejects if the current model differs; no geometry is written."},
                    "linear_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.15},
                    "angular_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.35}
                }),
                &[],
            ),
        ),
        ToolSpec::direct(
            "solid_export_3mf",
            "Export 3MF",
            "Tessellate active bodies into a standard 3MF (mm, portable material name, display color). Prusa and Cura add that slicer's metadata. Bambu and Orca stay the standard model, not a sliced project.",
            "solid_export_3mf",
            Payload::Object,
            object_schema(
                json!({
                    "body_ids": {
                        "type": "array",
                        "items": {"type": "integer", "minimum": 1},
                        "description": "Empty exports every active body."
                    },
                    "named_view":{"type":"string","description":"Saved layout name; requires assembly scope. Includes every visible repetition."},
                    "print_bed":print_bed_schema(),
                    "scope": {"type":"string","enum":["assembly","definition"],"default":"assembly","description":"Assembly exports visible solved occurrences. Definition exports each selected body once in its part coordinates."},
                    "expected_model_json": {"type":"string","description":"Optional exact cad_project_model string captured before an interactive choice. Export rejects if the current model differs; no geometry is written."},
                    "linear_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.15},
                    "angular_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.35},
                    "include_appearance": {"type": "boolean", "default": true},
                    "slicer_target": {
                        "type": "string",
                        "enum": ["standard", "bambu_studio", "orca_slicer", "prusa_slicer", "cura"],
                        "default": "standard",
                        "description": "standard is the portable 3MF (mesh, material name, display color). Named slicer targets add that slicer's own metadata and are not a sliced print."
                    }
                }),
                &[],
            ),
        ),
        ToolSpec::direct(
            "printer_catalog",
            "Printer catalog",
            "Return embedded pinned printer beds, nozzle modes and source provenance for presentation and print layouts.",
            "printer_catalog",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "material_catalog",
            "Material catalog",
            "Return the unified embedded plastic, metal, and filament catalog with engineering properties, print profiles, units, source provenance, and data quality notes.",
            "material_catalog",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "body_appearances",
            "List body appearances",
            "Return per-body color/filament assignments used by 3MF export and the viewport.",
            "body_appearances",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "set_body_appearance",
            "Set body appearance",
            "Assign material/color to a body. Prefer body_id + preset_id from material_catalog; or pass a full BodyAppearance including its frozen material property snapshot.",
            "set_body_appearance",
            Payload::BodyAppearance,
            object_schema(
                json!({
                    "body_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Target body id from solid_scene."
                    },
                    "preset_id": {
                        "type": "string",
                        "description": "Catalog material id (e.g. material.aluminum-6061-t6 or bambu.pla.basic.red). Shorthand resolves the catalog; a full material snapshot preserves saved properties."
                    },
                    "color": {
                        "type": "object",
                        "properties": {
                            "r": {"type": "integer", "minimum": 0, "maximum": 255},
                            "g": {"type": "integer", "minimum": 0, "maximum": 255},
                            "b": {"type": "integer", "minimum": 0, "maximum": 255},
                            "a": {"type": "integer", "minimum": 0, "maximum": 255}
                        }
                    },
                    "material_name": {"type": "string"},
                    "filament_type": {"type": "string"},
                    "brand": {"type": "string"},
                    "color_name": {"type": "string"},
                    "filament_id": {"type": ["string", "null"]},
                    "density_g_cm3": {"type": ["number", "null"]},
                    "diameter_mm": {"type": "number", "exclusiveMinimum": 0},
                    "material": {
                        "type": ["object", "null"],
                        "description": "Frozen merged MaterialDetails from material_catalog or body_appearances. Pass the complete saved object to preserve its properties. Property source_id values must refer to sources in this snapshot.",
                        "properties": {
                            "kind": {"type":"string","enum":["plastic","metal"]},
                            "catalog_id": {"type":"string"},
                            "properties": {"type":"array","items":{"type":"object"}},
                            "sources": {"type":"array","items":{"type":"object"}},
                            "print_profiles": {"type":"array","items":{"type":"object"}},
                            "warnings": {"type":"array","items":{"type":"string"}}
                        },
                        "required":["kind","catalog_id"]
                    }
                }),
                &["body_id"],
            ),
        ),
        ToolSpec::direct(
            "solid_export_preflight",
            "Export preflight",
            "Check timeline errors, appearance coverage and assembly print layout before export. Reports included/excluded quantities, conservative overlaps and envelope issues, plus additional whole-group translations to review. Warnings permit deliberate export. Defaults to the Bambu X2D main envelope.",
            "solid_export_preflight",
            Payload::Object,
            object_schema(json!({
                "named_view":{"type":"string","description":"Omit for the current displayed view; empty selects assembled placement; a name selects its saved snapshot."},
                "body_ids":{"type":"array","items":{"type":"integer","minimum":1}},
                "expected_model_json":{"type":"string"},
                "print_bed":print_bed_schema()
            }), &[]),
        ),
        ToolSpec::direct(
            "demo_export_pip_3mf",
            "Export PIP demo 3MF",
            "Return a built-in print-in-place demo as base64 3MF (AABB clearance smoke â‰¥ 0.4 mm). Does not mutate the document. kind=cam_bolt (default, 4-body wedge+dial) or clip (3-body drawer).",
            "demo_export_pip_3mf",
            Payload::Object,
            object_schema(
                json!({
                    "kind": {
                        "type": "string",
                        "enum": ["cam_bolt", "clip"],
                        "default": "cam_bolt"
                    },
                    "slicer_target": {
                        "type": "string",
                        "enum": ["standard", "bambu_studio", "orca_slicer", "prusa_slicer", "cura"],
                        "default": "standard"
                    }
                }),
                &[],
            ),
        ),
        ToolSpec::control(
            "solid_box",
            "Create box",
            "Create a rectangular block from an origin corner and a size, as ordinary editable history: an offset plane when the origin is off the XY plane, a dimensioned rectangle fixed at its origin corner, and one extrude (new_body, join, cut or intersect). Returns the feature id, body ids and a feature summary. Use it for stock, plates and blocks instead of a five-step sketch.",
            object_schema(
                json!({
                    "origin": {"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3,"description":"Lower-left-bottom corner [x, y, z] in mm; default [0, 0, 0]"},
                    "size": {"type":"array","items":{"type":"number","exclusiveMinimum":0},"minItems":3,"maxItems":3,"description":"Extents [length x, width y, height z] in mm"},
                    "operation": {"type":"string","enum":["new_body","join","cut","intersect"],"default":"new_body"},
                    "target_body_ids": {"type":"array","items":{"type":"integer","minimum":1},"description":"Bodies a join, cut or intersect applies to"},
                    "name": {"type":"string","minLength":1,"description":"Sketch name kept in history"}
                }),
                &["size"],
            ),
        ),
        ToolSpec::control(
            "print_calibrate",
            "Locate the plate on a scanned print",
            "Find the plate's plan-view outline on a scanned 2D print (PDF, PNG or PGM) from its length and width and report the calibration: corners, pixels per millimetre and skew. The outline is chosen by aspect ratio and line weight (visible outlines are drawn heavier than dimension lines and table rules). Pass hint as \"x0,y0,x1,y1\" page fractions around the plan view when a sheet defeats the search.",
            print_schema(json!({})),
        ),
        ToolSpec::control(
            "print_crop",
            "Millimetre crop of a print with a grid and hole overlay",
            "Render a millimetre window of the print (plan-view frame: origin at the plate's lower-left corner, y up) with green tick marks every grid_mm and, by default, the current document's holes drawn in red (counterbores blue), returned as image content and optionally written to out_png. Positions can be read off the ticks; every red circle must sit on a drawn hole symbol.",
            print_schema(json!({
                "region": {"type":"string","description":"\"x0,y0,x1,y1\" in plate mm"},
                "dpi": {"type":"integer","minimum":72,"maximum":1600,"default":400},
                "grid_mm": {"type":"number","minimum":0,"default":10},
                "out_png": {"type":"string","description":"Absolute path to also write the PNG to"}
            })),
        ),
        ToolSpec::control(
            "print_probe",
            "What is drawn at each model hole",
            "For every hole (the document's by default) say what the print shows at that point or within search_mm of it: symbol (a circle with a light interior, with its centre, the offset in mm and the drawn diameter and counterbore), dot (a solid dot), dashed (a partial ring such as a hidden-line circle) or none. Run it after every script run and look at every hole with an offset over 1 mm or nothing drawn.",
            print_schema(json!({
                "search_mm": {"type":"number","exclusiveMinimum":0,"default":2.5},
                "dpi": {"type":"integer","minimum":72,"maximum":1600,"default":600}
            })),
        ),
        ToolSpec::control(
            "print_symbols",
            "Drawn hole symbols and coverage",
            "List the circles and solid dots found at ink crossings in a region of the print and match them to the model's holes: model_only are holes with no drawn symbol, print_only are drawn symbols with no hole. Text, arrowheads and concentric rings can still appear in print_only, so treat those entries as places to look at on a crop, never as positions to model from. With draw or out_png the match is returned as an image.",
            print_schema(json!({
                "region": {"type":"string","description":"\"x0,y0,x1,y1\" in plate mm; default the whole plate"},
                "dpi": {"type":"integer","minimum":72,"maximum":1600,"default":400},
                "draw": {"type":"boolean","default":false},
                "out_png": {"type":"string"}
            })),
        ),
        ToolSpec::control(
            "cad_get_focus",
            "Get focus state",
            "Return the active focus pack, soft packs, TTLs, and disclosure mode.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_set_focus",
            "Set focus",
            "Set the active modeling focus pack and schedule a throttled tools/list_changed notification.",
            object_schema(
                json!({
                    "focus": {
                        "type": "string",
                        "enum": ["document", "assembly", "sketch", "solid", "modify", "body_ops", "datums", "history", "inspect", "print", "cam"]
                    },
                    "explicit": {
                        "type": "boolean",
                        "description": "When true, auto-focus hints are ignored until cleared."
                    }
                }),
                &["focus"],
            ),
        ),
        ToolSpec::control(
            "cad_list_focus_areas",
            "List focus areas",
            "Return the supported focus packs and human-readable descriptions.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_get_tool_disclosure_mode",
            "Get disclosure mode",
            "Return the current tool disclosure mode: dynamic or full_static.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_set_tool_disclosure_mode",
            "Set disclosure mode",
            "Switch between dynamic focus-scoped advertisement and the full_static escape hatch.",
            object_schema(
                json!({
                    "mode": {
                        "type": "string",
                        "enum": ["dynamic", "full_static"]
                    }
                }),
                &["mode"],
            ),
        ),
        ToolSpec::control(
            "cad_list_all_tools",
            "List full tool catalog",
            "Return every registered tool with schemas and focus tags without changing advertisement.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_help",
            "Search and read local help",
            "One help surface over the bundled knowledge corpus (machine-design + agent doctrine). Actions: search (snippet-first, default limit 5 max 10), get (id-only allowlist, 12KiB cap), topics (page size 50). Prefer cad_help before web search. Recipe chips on pages deep-link Scripts/presentation â€” no Bevy-in-Help.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["search", "get", "topics"],
                        "description": "search | get | topics"
                    },
                    "query": {
                        "type": "string",
                        "description": "Required for action=search"
                    },
                    "id": {
                        "type": "string",
                        "description": "Help page id from search hits; required for action=get. Paths rejected."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 10,
                        "description": "search hit limit (default 5, max 10)"
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "topics listing offset"
                    }
                }),
                &["action"],
            ),
        ),

        ToolSpec::control(
            "cad_cancel_recompute",
            "Cancel solid recompute",
            "Abort an in-flight solid replay if one is pending in this MCP process.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_list_sessions",
            "List read-only session snapshots",
            "List UUID v4 session directories under LIMO_CAD_SESSION_DIR (skips _* control dirs and non-UUID names). Includes stable window_id / document_id when the UI publisher wrote them, heartbeat age/stale metadata, expiring desktop process leases, and a windows[] projection with authoritative active documents. Use with cad_attach. Snapshot bridge â€” not a live UI co-link. Stdio headless sessions without UI identity still list.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_computer_control", "Operate the native CAD window",
            computer_control::description(),
            computer_control::schema(),
        ),
        ToolSpec::control(
            "cad_interface", "Explore and drive the product interface",
            "Catalog returns shared product groups and typed operations. Execute runs an operation by group and name with identical arguments/results headlessly or live. Recipes lists committed native examples without running them. Open_recipe queues a built-in recipe in the live Scripts source editor, preserving edited source with Save/Discard/Cancel; it never runs commands or replaces the model. Script runs one versioned JSONC command file selected by recipe ID, source or an absolute .limo.jsonc path in the current blank document; Rust sequences every operation, stops on failure, and runs final checks by default. Its result carries a feature summary (bodies with bounding boxes, holes tallied by class; detail full lists every hole with position, diameter, depth, face and thread) and warnings for mistakes that raise no error: a hole position left out of positions, overlapping holes, holes off the body, blind depths deeper than the body, unused bindings; a failing step names the step, the reason and, for selectors, the candidates or the values present. Summary returns that feature summary of the current document (detail compact or full). Check compares expected {bbox: [x, y, z], holes: [{x, y, z?, diameter?, counterbore_diameter?, through?, depth?}]} with the built model within tolerance_mm (default 0.6) and reports matched, missing and extra holes with offsets. Export_script emits a version-1 .limo.jsonc from the last successful script source (from last_script, fidelity lossless_authored) or from the session tool_trace (from session_trace, fidelity lossy_session_trace); it is distinct from cad_script's forward call dump. Mode fast has no presentation delays; present requires an attached desktop. Presentation provides configure/note/pause/resume/step/stop/status/finish/dismiss/show and speed controls shared with native playback. View supports timed orientation and focus on an active sketch, body, or component. Launch connects a new desktop. History with command undo or redo uses the desktop document history controller; inspect state.history reports availability. Inspect returns rendered controls with fresh opaque target IDs for click/set_value/key. Window close requests guarded application exit; the reply acknowledges the request, not process termination. No selectors or executable script evaluation.",
            interface::with_file_options(object_schema(json!({
                "session_id":{"type":"string"},
                "action":{"type":"string","enum":["catalog","recipes","open_recipe","execute","script","summary","check","export_script","presentation","launch","view","inspect","capture","click","double_click","context_menu","set_value","key","window","file","history","viewport"]},
                "recipe":{"type":"string","description":"Bundled recipe ID for script or open_recipe; mutually exclusive with source and path. List IDs with action recipes."},
                "group":{"type":"string"},"operation":{"type":"string"},"arguments":{"type":"object"},
                "executable":{"type":"string"},
                "view":{"type":"string","enum":["current","isometric","top","bottom","front","back","left","right"]},
                "fit":{"type":"boolean"},
                "body_id":{"type":"integer","minimum":0},"component_id":{"type":"integer","minimum":0},
                "duration_ms":{"type":"integer","minimum":0,"maximum":10000},
                "orbit_degrees":{"type":"number","minimum":-360,"maximum":360,"description":"For action view with view current: rotate about the current camera target/up axis at fixed radius and elevation. Optional fit/focal target frames first. Positive angles turn counterclockwise viewed along the up axis toward the target."},
                "source":{"type":"string","description":"Version 1 JSONC command script; mutually exclusive with path"},
                "include_base":{"type":"string","description":"Absolute directory that relative includes resolve under. Inline source only; not valid with path or recipe, whose includes resolve beside the file."},
                "validate":{"type":"boolean","default":true},
                "speed":{"type":"number","minimum":0.1,"maximum":16},
                "text":{"type":"string","maxLength":4000},"chapter":{"type":"string","maxLength":200},
                "step_index":{"type":"integer","minimum":0},"step_count":{"type":"integer","minimum":0},
                "gesture":{"type":"string","enum":["move","click","double_click","drag"]},
                "canvas":{"type":"string","enum":["viewport","drawing"],"description":"Defaults to the visible drawing paper when present, otherwise the model viewport. Drawing gestures use client point/to coordinates from inspect."},
                "button":{"type":"string","enum":["left","middle"],"default":"left","description":"Middle-button drag pans drawing paper or the model camera through the native navigation handler."},
                "to":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
                "point":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
                "world":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},
                "shift":{"type":"boolean"},
                "command":{"type":"string","enum":["open","save","rename","undo","redo","configure","note","pause","resume","step","stop","status","finish","dismiss","show"]},
                "path":{"type":"string"},"name":{"type":"string"},
                "from":{"type":"string","enum":["auto","last_script","session_trace"],"description":"export_script source: last_script (authored, stale:true if tools ran after it), session_trace (lossy_session_trace), or auto (authored while no modeling tool has run since that script, live desktop edits included; otherwise the session trace)."},
                "overwrite":{"type":"boolean"},"discard_changes":{"type":"boolean"},
                "target":{"type":"string","description":"Fresh inspect control ID, or active_sketch for view"},"value":{"type":"string"},
                "key":{"type":"string","enum":["Enter","Escape","ArrowUp","ArrowDown","ArrowLeft","ArrowRight","Home","End","Delete","Backspace"]},
                "mode":{"type":"string","enum":["foreground","background","inspect","close","fast","present"]},
                "pace_ms":{"type":"integer","minimum":0,"maximum":2000}
            }), &[])),
        ),
        ToolSpec::control(
            "cad_attach",
            "Attach read-only session snapshot",
            "Load a published snapshot into this MCP process by session_id (UUID), window_id (stable desktop window id), and/or document_id (native project-session id; UUID still aliases session_id). Requires valid model.json; optional focus.json. Seeds cad_script baseline with cad_load_project_model (loaded model_json). Fails if the target/model is missing, invalid, or ambiguous. writeback must be omitted or false. While attached to a compatible desktop, the same operation calls submit and await live application internally. Older desktops reject before submission. Never writes back to the session dir. Headless goldens skip attach.",
            object_schema(
                json!({
                    "session_id": {
                        "type": "string",
                        "minLength": 36,
                        "maxLength": 36,
                        "description": "UUID v4 session directory name"
                    },
                    "window_id": {
                        "type": "string",
                        "minLength": 1,
                        "description": "Stable desktop window id published in heartbeat/focus"
                    },
                    "document_id": {
                        "type": "string",
                        "minLength": 1,
                        "description": "Native project-session / document id from heartbeat, or UUID alias for session_id"
                    },
                    "writeback": {
                        "type": "boolean",
                        "description": "Must be omitted or false. true is rejected; the live engine owns mutations while attached."
                    }
                }),
                &[],
            ),
        ),
        ToolSpec::control(
            "cad_refresh",
            "Refresh attached session snapshot",
            "Re-read model.json (and optional focus.json) for the currently attached session; replaces cad_script baseline with cad_load_project_model for the reloaded model. Explicit refresh â€” MCP does not watch the filesystem.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_detach",
            "Detach session snapshot",
            "Clear the attached session id. Leaves the in-memory document as last loaded; does not delete session files.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_script",
            "Dump forward MCP script",
            "Return this process's successful mutating tool-call sequence as JSON { calls: [{ name, arguments }] }. Portable modeling ops only â€” skips session-control reads (cad_attach/cad_refresh/cad_detach), inspect/export helpers, failed calls, and cad_script itself. After attach/refresh, the trace baseline is cad_load_project_model with the loaded model_json (refresh replaces that baseline). Does not reverse-engineer STEP feature history. For version-1 .limo.jsonc export see cad_interface action export_script.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_compare_solids",
            "Compare solid scene metrics",
            "Summarize active bodies from solid_scene: body count plus per-body bbox, vertex_count, and triangle_count from existing mesh fields. Attached calls inspect the owning desktop's live scene with owner/generation fences and retain its source receipt; headless calls read the existing local scene. No geometry replay or model mutation. Use to check a rebuilt history against an imported reference solid. Does not invent volume.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_submit",
            "Submit modeling op for UI-owned apply",
            "While attached, write one modeling mutate to inbox/<seq>.json. Does not mutate this MCP process. UI/engine applies via host::handle, then publishes a new snapshot. Rejects if not attached, if base_generation != heartbeat generation, or if the tool is inspect/export/control. Headless (no attach) still calls mutate tools directly. After submit, prefer cad_await_apply(seq) instead of racing cad_refresh.",
            object_schema(
                json!({
                    "name": {
                        "type": "string",
                        "minLength": 1,
                        "description": "MCP modeling tool name to apply on the live UI document"
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments for the named modeling tool"
                    },
                    "base_generation": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Heartbeat generation this op is based on"
                    }
                }),
                &["name", "base_generation"],
            ),
        ),
        ToolSpec::control(
            "cad_await_apply",
            "Await UI apply receipt for submitted inbox seq",
            "While attached, poll until inbox/applied/<seq>.json or inbox/failed/<seq>.json appears. For applied ops, also wait until an explicit published_generation catches up to the engine. Completed-model publications optionally cad_refresh (refresh default true); active-sketch-only publications return model_published:false, active_sketch_published:true, refreshed:false because model.json intentionally remains the last completed model. timeout_ms 0 is a single status probe. Still snapshot/UI-owned apply â€” not in-process co-link. Does not write model.json.",
            object_schema(
                json!({
                    "session_id": {
                        "type": "string",
                        "description": "Receipt owner returned by cad_submit. Pass it with seq across document/attachment changes. Defaults to the current attachment; reading another session's receipt never changes or refreshes this attachment."
                    },
                    "seq": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Inbox sequence returned by cad_submit"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Max wait in ms (default 5000, max 30000). 0 = single status probe"
                    },
                    "poll_ms": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Poll interval in ms (default 50)"
                    },
                    "refresh": {
                        "type": "boolean",
                        "description": "When status=applied and model_published=true, reload the completed model snapshot (default true); active-sketch-only snapshots are reported without reloading stale model.json"
                    }
                }),
                &["seq"],
            ),
        ),
        ToolSpec::control(
            "cad_session_status",
            "Report attached session status vs live publisher",
            "Report the loaded completed model generation against the live publisher, identity, publication fences, heartbeat age, pending operations, and latest receipt. A missing loaded generation is stale. Active-sketch publication is distinct from completed-model publication. Headless returns attached:false and code:not_attached. Observes without refreshing the model or changing the live document.",
            empty_schema(),
        ),
    ];
    tools.extend(drawing_tools::specs());
    tools.extend(broker::specs());
    tools.extend(assembly_tools::specs());
    tools.extend(cam_tools::specs());
    tools.extend(print_intent_tools::specs());
    tools.extend(print_height_tools::specs());
    tools.extend(print_modifier_tools::specs());
    let manufacturing = manufacturing_tools::specs();
    let project_schema = manufacturing
        .iter()
        .find(|tool| tool.name == "solid_export_bambu_project")
        .expect("Bambu export specification")
        .input_schema
        .clone();
    tools.extend(manufacturing);
    tools.extend(local_slicer_tools::specs(project_schema));
    for tool in &mut tools {
        let (pack, spine) = tags_for_tool(tool.name);
        tool.pack = pack;
        tool.spine = spine;
    }
    tools
}

fn records_in_script(name: &str) -> bool {
    if matches!(name, "cad_route" | "cad_computer_control") {
        return false;
    }
    if matches!(
        name,
        "bambu_template_inspect"
            | "bambu_project_preview"
            | "solid_export_bambu_project"
            | "bambu_local_verification_start"
            | "bambu_local_verification_poll"
            | "bambu_local_verification_cancel"
    ) {
        return false;
    }
    if name.starts_with("print_intent_") || name.starts_with("print_modifier_") {
        return false;
    }
    if limo_cad_mcp_mutate::lookup_mutate(name).is_some_and(|spec| spec.is_read_only()) {
        return false;
    }
    if matches!(
        name,
        "drawing_document"
            | "drawing_projection"
            | "drawing_export"
            | "cam_get_document"
            | "cam_toolpath_statuses"
    ) {
        return false;
    }
    if matches!(
        name,
        "cad_script"
            | "cad_interface"
            | "cad_compare_solids"
            | "cad_document"
            | "cad_project_model"
            | "cad_get_focus"
            | "cad_set_focus"
            | "cad_list_focus_areas"
            | "cad_get_tool_disclosure_mode"
            | "cad_set_tool_disclosure_mode"
            | "cad_list_all_tools"
            | "cad_help"
            | "cad_cancel_recompute"
            | "cad_list_sessions"
            | "cad_attach"
            | "cad_refresh"
            | "cad_detach"
            | "cad_submit"
            | "cad_await_apply"
            | "cad_session_status"
            | "sketch_active"
            | "sketch_finished"
            | "sketch_profiles"
            | "solid_scene"
            | "solid_tessellate"
            | "solid_export_step"
            | "solid_export_stl"
            | "solid_export_3mf"
            | "solid_export_preflight"
            | "printer_catalog"
            | "material_catalog"
            | "body_appearances"
            | "project_visibility"
            | "named_views"
            | "named_view_solution"
            | "set_named_views"
            | "upsert_named_view"
            | "rename_named_view"
            | "delete_named_view"
            | "recall_named_view"
            | "clear_named_view"
            | "demo_export_pip_3mf"
            | "print_calibrate"
            | "print_crop"
            | "print_probe"
            | "print_symbols"
    ) {
        return false;
    }
    if name.ends_with("_definitions") || name.contains("_preview_") {
        return false;
    }
    true
}

fn compare_solids_summary(scene: &limo_cad_solid::SolidSceneDto) -> Value {
    let bodies: Vec<Value> = scene
        .bodies
        .iter()
        .map(|body| {
            let positions = &body.mesh.positions;
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for chunk in positions.as_chunks::<3>().0 {
                for (i, component) in chunk.iter().enumerate() {
                    min[i] = min[i].min(*component);
                    max[i] = max[i].max(*component);
                }
            }
            let empty = positions.len() < 3;
            json!({
                "id": body.id,
                "name": body.name,
                "vertex_count": positions.len() / 3,
                "triangle_count": body.mesh.indices.len() / 3,
                "bbox_min": if empty {
                    Value::Null
                } else {
                    json!([min[0], min[1], min[2]])
                },
                "bbox_max": if empty {
                    Value::Null
                } else {
                    json!([max[0], max[1], max[2]])
                },
            })
        })
        .collect();
    json!({
        "body_count": bodies.len(),
        "bodies": bodies,
        "error_count": scene.errors.len(),
    })
}

fn tool_list_result(disclosure: &mut DisclosureState) -> Value {
    disclosure.tick_soft_expiry();
    Value::Object(Map::from_iter([(
        "tools".to_string(),
        Value::Array(
            tool_specs()
                .iter()
                .filter(|tool| disclosure.is_advertised(tool.name, tool.pack, tool.spine))
                .map(tool_entry)
                .collect(),
        ),
    )]))
}

fn success_result(value: Value) -> Value {
    let mut value = value;
    let image = value
        .pointer("/image/png_base64")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if image.is_some() {
        value["image"]["png_base64"] = json!("(attached as image content)");
    }
    let structured = if value.is_object() {
        value.clone()
    } else {
        json!({ "value": value.clone() })
    };
    let mut content = vec![json!({
        "type": "text",
        "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
    })];
    if let Some(png) = image {
        content.push(json!({"type": "image", "data": png, "mimeType": "image/png"}));
    }
    json!({
        "content": content,
        "structuredContent": structured,
        "isError": false
    })
}

fn tool_error(message: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

fn response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

fn handle_message(server: &mut CadServer, message: Value) -> Vec<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Vec::new();
    };
    let id = message.get("id").cloned();
    let mut responses = match method {
        "initialize" => {
            let requested = message
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(LATEST_PROTOCOL);
            let protocol = match requested {
                "2024-11-05" | "2025-03-26" | "2025-06-18" => requested,
                _ => LATEST_PROTOCOL,
            };
            vec![response(
                id.unwrap_or(Value::Null),
                json!({
                    "protocolVersion": protocol,
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "resources": { "subscribe": false, "listChanged": false },
                        "prompts": { "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "limo-cad",
                        "title": "Limo CAD",
                        "version": limo_cad_build_info::build_info().display_version(),
                        "_meta": {"limo-cad/build": limo_cad_build_info::build_info()}
                    },
                    "instructions": stdio::instructions(server.desktop_binding.is_some())
                }),
            )]
        }
        "notifications/initialized" | "notifications/cancelled" => Vec::new(),
        "ping" => id.map(|id| response(id, json!({}))).into_iter().collect(),
        "resources/list" => {
            let id = id.unwrap_or(Value::Null);
            if message
                .get("params")
                .is_some_and(|params| !params.is_null() && !params.is_object())
                || message
                    .pointer("/params/cursor")
                    .is_some_and(|cursor| !cursor.is_null())
            {
                vec![error_response(
                    id,
                    -32602,
                    "resources/list has no pagination cursor",
                )]
            } else {
                vec![response(id, knowledge::list())]
            }
        }
        "resources/read" => {
            let id = id.unwrap_or(Value::Null);
            match message.pointer("/params/uri").and_then(Value::as_str) {
                None => vec![error_response(
                    id,
                    -32602,
                    "resources/read requires params.uri",
                )],
                Some(uri) => match knowledge::read(uri) {
                    Some(contents) => vec![response(id, contents)],
                    None => vec![error_response(id, -32002, "knowledge resource not found")],
                },
            }
        }
        "prompts/list" => {
            let id = id.unwrap_or(Value::Null);
            if message
                .get("params")
                .is_some_and(|params| !params.is_null() && !params.is_object())
                || message
                    .pointer("/params/cursor")
                    .is_some_and(|cursor| !cursor.is_null())
            {
                vec![error_response(
                    id,
                    -32602,
                    "prompts/list has no pagination cursor",
                )]
            } else {
                vec![response(id, prompts::list())]
            }
        }
        "prompts/get" => {
            let id = id.unwrap_or(Value::Null);
            let Some(name) = message.pointer("/params/name").and_then(Value::as_str) else {
                return vec![error_response(
                    id,
                    -32602,
                    "prompts/get requires params.name",
                )];
            };
            let arguments = message.pointer("/params/arguments");
            match prompts::get(name, arguments) {
                Ok(result) => vec![response(id, result)],
                Err(message) => vec![error_response(id, -32602, message)],
            }
        }
        "tools/list" => vec![response(
            id.unwrap_or(Value::Null),
            tool_list_result(&mut server.disclosure),
        )],
        "tools/call" => {
            let id = id.unwrap_or(Value::Null);
            let Some(name) = message.pointer("/params/name").and_then(Value::as_str) else {
                return vec![error_response(
                    id,
                    -32602,
                    "tools/call is missing params.name",
                )];
            };
            let arguments = message
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !tool_specs().iter().any(|tool| tool.name == name) {
                return vec![error_response(id, -32602, format!("unknown tool: {name}"))];
            }
            let result = match server.call_tool(name, arguments) {
                Ok(value) => success_result(value),
                Err(error) => tool_error(error),
            };
            vec![response(id, result)]
        }
        _ if id.is_none() => Vec::new(),
        _ => vec![error_response(
            id.unwrap_or(Value::Null),
            -32601,
            format!("method not found: {method}"),
        )],
    };
    if let Some(notification) = server.disclosure.take_notify_if_due() {
        responses.push(notification);
    }
    responses
}

/// Emit due soft-TTL / list_changed notifications without waiting for another
/// client RPC. Used by the stdin+timeout worker (Jack Â§2) and by unit tests.
fn idle_due_messages(server: &mut CadServer) -> Vec<Value> {
    server.disclosure.tick_soft_expiry();
    let mut outgoing = Vec::new();
    if let Some(notification) = server.disclosure.take_notify_if_due() {
        outgoing.push(notification);
    }
    outgoing
}

fn help_store() -> &'static limo_cad_help::HelpStore {
    use std::sync::OnceLock;
    static STORE: OnceLock<limo_cad_help::HelpStore> = OnceLock::new();
    STORE.get_or_init(limo_cad_help::HelpStore::bundled)
}

fn cad_help_call(arguments: &Value) -> Result<Value, String> {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing required argument 'action'".to_string())?;
    let store = help_store();
    match action {
        "search" => {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "search requires 'query'".to_string())?;
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .map(|n| n as usize);
            let hits = store.search(query, limit);
            Ok(json!({
                "action": "search",
                "query": query,
                "limit": limit.unwrap_or(limo_cad_help::SEARCH_DEFAULT_LIMIT).clamp(1, limo_cad_help::SEARCH_MAX_LIMIT),
                "hits": hits.iter().map(|h| json!({
                    "id": h.id,
                    "title": h.title,
                    "topics": h.topics,
                    "snippet": h.snippet,
                    "score": h.score,
                    "related_recipes": h.related_recipes,
                    "status": h.status,
                })).collect::<Vec<_>>(),
            }))
        }
        "get" => {
            let id = arguments
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "get requires 'id'".to_string())?;
            let page = store.get(id)?;
            Ok(json!({
                "action": "get",
                "id": page.id,
                "title": page.title,
                "topics": page.topics,
                "keywords": page.keywords,
                "description": page.description,
                "body": page.body,
                "related_recipes": page.related_recipes,
                "status": page.status,
                "truncated": page.truncated,
            }))
        }
        "topics" => {
            let offset = arguments
                .get("offset")
                .and_then(Value::as_u64)
                .map(|n| n as usize);
            let mut value = store.topics(offset);
            if let Some(obj) = value.as_object_mut() {
                obj.insert("action".into(), json!("topics"));
            }
            Ok(value)
        }
        other => Err(format!(
            "unknown cad_help action '{other}' (expected search|get|topics)"
        )),
    }
}

#[cfg(test)]
mod tests {
    mod print_intent;
    use super::*;
    mod cam_query_effects;

    #[test]
    fn dimensioned_rectangle_rejects_misspelled_drivers_without_mutating_the_sketch() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            )
            .unwrap();
        let before = server.call_tool("sketch_active", json!({})).unwrap();
        let schema = &tool_specs()
            .iter()
            .find(|tool| tool.name == "sketch_add_rectangle_locked")
            .unwrap()
            .input_schema;
        let request = json!({
            "mode":"two_point", "anchor":{"x":-30.,"y":-20.},
            "corner_hint":{"x":30.,"y":20.}, "ctrl_held":true
        });
        for name in ["width", "height"] {
            let mut invalid = request.clone();
            invalid[name] = json!(60.);
            assert!(schema_accepts(schema, &invalid).is_err());
            let error = server
                .call_tool("sketch_add_rectangle_locked", invalid)
                .unwrap_err();
            assert!(
                error.to_string().contains(name),
                "unknown rectangle driver must be identified"
            );
            assert_eq!(
                server.call_tool("sketch_active", json!({})).unwrap(),
                before
            );
        }
        for drivers in [
            json!({"width_mm":60.,"height_mm":40.}),
            json!({"width_text":"60","height_text":"40"}),
        ] {
            let mut valid = request.clone();
            valid
                .as_object_mut()
                .unwrap()
                .extend(drivers.as_object().unwrap().clone());
            schema_accepts(schema, &valid).unwrap();
            let result = server
                .call_tool("sketch_add_rectangle_locked", valid)
                .unwrap();
            assert_eq!(result["sketch"]["dimensions"].as_array().unwrap().len(), 2);
            assert_eq!(result["sketch"]["dof"]["value"], 2);
            server.call_tool("sketch_undo", json!({})).unwrap();
            assert!(
                server.call_tool("sketch_active", json!({})).unwrap()["entities"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn sketch_transform_keeps_requested_moves_and_rejects_conflicting_scale_atomically() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            )
            .unwrap();
        let rectangle = server
            .call_tool(
                "sketch_add_rectangle_locked",
                json!({
                    "mode":"two_point", "anchor":{"x":5.,"y":0.}, "corner_hint":{"x":10.,"y":12.},
                    "width_mm":5., "height_mm":12., "ctrl_held":true
                }),
            )
            .unwrap();
        assert!(rectangle["sketch"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entity| entity["fully_defined"] == false));
        let point = rectangle["sketch"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entity| {
                entity["kind"] == "point" && entity["position"] == json!({"x":5.,"y":0.})
            })
            .unwrap()["id"]
            .clone();
        let moved = server
            .call_tool(
                "sketch_move_copy",
                json!({"entity_ids":[point],"dx":1.,"dy":2.,"copy":false}),
            )
            .unwrap();
        let position = |sketch: &Value| {
            sketch["entities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entity| entity["id"] == point)
                .unwrap()["position"]
                .clone()
        };
        let target = position(&moved["sketch"]);
        assert!((target["x"].as_f64().unwrap() - 6.).abs() < 1e-8);
        assert!((target["y"].as_f64().unwrap() - 2.).abs() < 1e-8);
        assert_eq!(moved["sketch"]["dof"]["value"], 2);
        assert!(moved["sketch"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entity| entity["fully_defined"] == false));
        let undone = server.call_tool("sketch_undo", json!({})).unwrap();
        assert_eq!(position(&undone["sketch"]), json!({"x":5.,"y":0.}));
        let lines: Vec<_> = undone["sketch"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entity| entity["kind"] == "line")
            .map(|entity| entity["id"].clone())
            .collect();
        let before = server.call_tool("sketch_active", json!({})).unwrap();
        assert_eq!(before["can_redo"], true);
        server
            .call_tool(
                "sketch_move_copy",
                json!({"entity_ids":[point],"dx":0.,"dy":0.,"copy":false}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_scale",
                json!({"entity_ids":lines,"origin":{"x":123.4,"y":-456.7},"factor_text":"1"}),
            )
            .unwrap();
        assert_eq!(
            server.call_tool("sketch_active", json!({})).unwrap(),
            before
        );
        let rejected = server
            .call_tool(
                "sketch_scale",
                json!({"entity_ids":lines,"origin":{"x":0.,"y":0.},"factor_text":"2"}),
            )
            .unwrap_err();
        assert!(
            rejected.contains("conflicts"),
            "conflicting scale must report the constraint conflict"
        );
        assert_eq!(
            server.call_tool("sketch_active", json!({})).unwrap(),
            before
        );
        let redone = server.call_tool("sketch_redo", json!({})).unwrap();
        assert_eq!(position(&redone["sketch"]), target);
        server
            .call_tool(
                "sketch_add_constraint",
                json!({"type":"fix","entity":point}),
            )
            .unwrap();
        let fixed = server.call_tool("sketch_active", json!({})).unwrap();
        assert_eq!(fixed["dof"]["value"], 0);
        assert!(fixed["entities"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entity| entity["fully_defined"] == true));
        assert!(server
            .call_tool(
                "sketch_move_copy",
                json!({"entity_ids":[point],"dx":1.,"dy":0.,"copy":false})
            )
            .unwrap_err()
            .contains("conflicts"));
        assert_eq!(server.call_tool("sketch_active", json!({})).unwrap(), fixed);
        let undone = server.call_tool("sketch_undo", json!({})).unwrap();
        assert_eq!(undone["sketch"]["dof"]["value"], 2);
        assert_eq!(position(&undone["sketch"]), target);
    }

    #[test]
    fn sketch_transform_preserves_curve_parameters_and_shared_handles() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            )
            .unwrap();
        server.call_tool("sketch_add_circle", json!({"mode":"center_diameter","p1":{"x":10.,"y":10.},"p2":{"x":15.,"y":10.},"ctrl_held":true})).unwrap();
        server.call_tool("sketch_add_arc_3pt", json!({"p1":{"x":20.,"y":0.},"p2":{"x":25.,"y":5.},"p3":{"x":30.,"y":0.},"ctrl_held":true})).unwrap();
        server
            .call_tool(
                "sketch_add_spline",
                json!({"points":[{"x":40.,"y":0.},{"x":45.,"y":5.},{"x":50.,"y":0.}]}),
            )
            .unwrap();
        let before = server.call_tool("sketch_active", json!({})).unwrap();
        let curves: Vec<_> = before["entities"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entity| matches!(entity["kind"].as_str(), Some("circle" | "arc" | "spline")))
            .map(|entity| entity["id"].clone())
            .collect();
        assert_eq!(curves.len(), 3);
        let moved = server
            .call_tool(
                "sketch_move_copy",
                json!({"entity_ids":curves,"dx":3.,"dy":-2.,"copy":false}),
            )
            .unwrap();
        assert_eq!(moved["sketch"]["dof"], before["dof"]);
        for entity in before["entities"].as_array().unwrap() {
            let actual = moved["sketch"]["entities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|candidate| candidate["id"] == entity["id"])
                .unwrap();
            for field in ["position", "center"] {
                if entity[field].is_object() {
                    assert!(
                        (actual[field]["x"].as_f64().unwrap()
                            - entity[field]["x"].as_f64().unwrap()
                            - 3.)
                            .abs()
                            < 1e-8
                    );
                    assert!(
                        (actual[field]["y"].as_f64().unwrap()
                            - entity[field]["y"].as_f64().unwrap()
                            + 2.)
                            .abs()
                            < 1e-8
                    );
                }
            }
            for field in ["radius", "start_angle", "end_angle"] {
                if entity[field].is_number() {
                    assert_eq!(actual[field], entity[field]);
                }
            }
            if entity["kind"] == "spline" {
                for (actual, original) in actual["points"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(entity["points"].as_array().unwrap())
                {
                    assert!(
                        (actual["x"].as_f64().unwrap() - original["x"].as_f64().unwrap() - 3.)
                            .abs()
                            < 1e-8
                    );
                    assert!(
                        (actual["y"].as_f64().unwrap() - original["y"].as_f64().unwrap() + 2.)
                            .abs()
                            < 1e-8
                    );
                }
            }
        }
        let scaled = server
            .call_tool(
                "sketch_scale",
                json!({"entity_ids":curves,"origin":{"x":0.,"y":0.},"factor_text":"2"}),
            )
            .unwrap();
        assert_eq!(scaled["sketch"]["dof"], before["dof"]);
        for id in curves {
            let original = moved["sketch"]["entities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entity| entity["id"] == id)
                .unwrap();
            let actual = scaled["sketch"]["entities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entity| entity["id"] == id)
                .unwrap();
            if original["center"].is_object() {
                for axis in ["x", "y"] {
                    assert!(
                        (actual["center"][axis].as_f64().unwrap()
                            - original["center"][axis].as_f64().unwrap() * 2.)
                            .abs()
                            < 1e-8
                    );
                }
                assert_eq!(
                    actual["radius"].as_f64().unwrap(),
                    original["radius"].as_f64().unwrap() * 2.
                );
            }
            if original["kind"] == "spline" {
                for (actual, original) in actual["points"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(original["points"].as_array().unwrap())
                {
                    for axis in ["x", "y"] {
                        assert!(
                            (actual[axis].as_f64().unwrap()
                                - original[axis].as_f64().unwrap() * 2.)
                                .abs()
                                < 1e-8
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn section_review_is_a_shared_read_with_registered_modeling_disclosure() {
        let mut server = CadServer::new().unwrap();
        extrude_offset_box(&mut server, "Sketch1", 10.0, 30.0);
        let body = server.manager.solid_scene().bodies[0].id;
        let before = server.manager.export_project_model().unwrap();
        let review = server
            .call_tool(
                "solid_section_review",
                json!({"body_id":body,"plane":"xy","offset_mm":5.0,"probe_mm":0.0}),
            )
            .unwrap();
        assert!(review["svg"].as_str().unwrap().starts_with("<svg"));
        assert_eq!(review["outcome"], "material_section");
        assert!(review["cutaway"].is_null());
        assert!((review["probe_spans"][0]["length_mm"].as_f64().unwrap() - 20.).abs() < 1e-6);
        for offset in [0., 8.] {
            for keep_positive in [false, true] {
                let boundary = server
                    .call_tool(
                        "solid_section_review",
                        json!({
                            "body_id":body,"plane":"xy","offset_mm":offset,"probe_mm":0.,
                            "include_cutaway":true,"keep_positive":keep_positive
                        }),
                    )
                    .unwrap();
                assert_eq!(boundary["outcome"], "boundary_contact");
                assert!(boundary["probe_spans"].as_array().unwrap().is_empty());
                assert!(boundary["cutaway"].is_null());
            }
        }
        assert_eq!(server.manager.export_project_model().unwrap(), before);
        assert!(is_read_safe_while_attached("solid_section_review"));
        assert!(limo_cad_mcp_mutate::is_live_engine_query(
            "solid_section_review"
        ));
        assert!(!changes_model("solid_section_review"));
        assert!(!is_modeling_mutate("solid_section_review"));
        assert_eq!(
            interface::group_for("solid_section_review"),
            Some("solid/check")
        );
        assert!(server
            .call_tool(
                "solid_section_review",
                json!({"body_id":body,"plane":"xy","offset_mm":5.,"deflection_mm":1.})
            )
            .is_err());
    }

    #[test]
    fn drawing_exports_current_associative_geometry_bom_and_rejects_stale_edits() {
        let mut server = CadServer::new().unwrap();
        extrude_offset_box(&mut server, "Sketch1", 10.0, 30.0);
        server
            .call_tool(
                "drawing_create_sheet",
                json!({"name":"Fixture","format":"a4","orientation":"landscape"}),
            )
            .unwrap();
        server.call_tool("drawing_add_view",json!({"sheet_id":1,"view":{"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[90.,65.],"scale":2.}})).unwrap();
        let projection = server
            .call_tool(
                "drawing_projection",
                json!({"direction":[0.,0.,1.],"up":[0.,1.,0.]}),
            )
            .unwrap();
        let anchors = projection["anchors"].as_array().unwrap();
        let first = &anchors[0];
        let second = anchors
            .iter()
            .find(|a| a["point"][0] != first["point"][0] && a["point"][1] != first["point"][1])
            .unwrap();
        let as_ref = |a: &Value| json!({"body_id":a["body_id"],"edge_id":a["edge_id"],"edge_key":a["edge_key"],"endpoint":a["endpoint"],"fallback_point":a["model_point"]});
        let dim = json!({"sheet_id":1,"view_id":1,"first":as_ref(first),"second":as_ref(second),"mode":"horizontal","offset":15.,"presentation":{"tolerance":{"mode":"symmetric","upper":0.2,"lower":-0.2}}});
        server
            .call_tool("drawing_add_linear_dimension", dim.clone())
            .unwrap();
        let prior = server.call_tool("drawing_document", json!({})).unwrap();
        let mut stale = dim;
        stale["first"]["edge_key"] = json!("removed edge");
        assert!(server
            .call_tool("drawing_add_linear_dimension", stale)
            .is_err());
        assert_eq!(
            prior,
            server.call_tool("drawing_document", json!({})).unwrap()
        );
        server.call_tool("drawing_set_bom",json!({"sheet_id":1,"items":[{"item_number":"1","body_id":first["body_id"],"part_number":"RAIL","description":"Printed test rail","quantity":1.,"material":"PETG","finish":"Fit coupon required"}],"position":[15.,120.]})).unwrap();
        let before_bom = server.call_tool("drawing_document", json!({})).unwrap();
        assert!(server.call_tool("drawing_set_bom",json!({"sheet_id":1,"items":[{"item_number":"1","part_number":"INVALID","description":"Negative quantity","quantity":-1.}]})).is_err());
        assert_eq!(
            before_bom,
            server.call_tool("drawing_document", json!({})).unwrap()
        );
        let svg = server
            .call_tool("drawing_export", json!({"sheet_id":1,"format":"svg"}))
            .unwrap();
        assert!(svg["content"]
            .as_str()
            .unwrap()
            .contains(">20.00 mm ±0.20</text>"));
        assert!(svg["content"]
            .as_str()
            .unwrap()
            .contains("Printed test rail"));
        assert_eq!(
            svg,
            server
                .call_tool("drawing_export", json!({"sheet_id":1,"format":"svg"}))
                .unwrap()
        );
        let dxf = server
            .call_tool("drawing_export", json!({"sheet_id":1,"format":"dxf"}))
            .unwrap();
        assert!(dxf["content"]
            .as_str()
            .unwrap()
            .contains("$INSUNITS\n70\n4"));
        let x_arm = anchors
            .iter()
            .find(|a| a["point"][0] != first["point"][0] && a["point"][1] == first["point"][1])
            .unwrap();
        let y_arm = anchors
            .iter()
            .find(|a| a["point"][0] == first["point"][0] && a["point"][1] != first["point"][1])
            .unwrap();
        server.call_tool("drawing_add_angular_dimension",json!({"sheet_id":1,"view_id":1,"vertex":as_ref(first),"first":as_ref(x_arm),"second":as_ref(y_arm),"radius":8.})).unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            )
            .unwrap();
        server.call_tool("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":50.,"y":0.},"p2":{"x":60.,"y":0.},"ctrl_held":true})).unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        server.call_tool("solid_extrude",json!({"sketch_name":"Sketch2","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":8.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]})).unwrap();
        let projection = server
            .call_tool(
                "drawing_projection",
                json!({"direction":[0.,0.,1.],"up":[0.,1.,0.]}),
            )
            .unwrap();
        let circle = &projection["circles"][0];
        let radial = json!({"sheet_id":1,"view_id":1,"feature":{"body_id":circle["body_id"],"edge_id":circle["edge_id"],"edge_key":circle["edge_key"],"fallback_center":circle["center_model"],"fallback_normal":circle["normal_model"],"fallback_radius":circle["radius"],"closed":circle["closed"]},"mode":"diameter","leader_angle_deg":45.,"offset":12.});
        server
            .call_tool("drawing_add_radial_dimension", radial.clone())
            .unwrap();
        let before = server.call_tool("drawing_document", json!({})).unwrap();
        let mut stale = radial;
        stale["feature"]["edge_key"] = json!("removed circle");
        assert!(server
            .call_tool("drawing_add_radial_dimension", stale)
            .is_err());
        assert_eq!(
            before,
            server.call_tool("drawing_document", json!({})).unwrap()
        );
        let svg = server
            .call_tool("drawing_export", json!({"sheet_id":1,"format":"svg"}))
            .unwrap();
        assert!(svg["content"].as_str().unwrap().contains("90.00°"));
        let diameter = circle["radius"].as_f64().unwrap() * 2.;
        assert!(svg["content"]
            .as_str()
            .unwrap()
            .contains(&format!("Ø{diameter:.2}")));
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        {
            let mut legacy: Value = serde_json::from_str(model.as_str().unwrap()).unwrap();
            assert_eq!(
                legacy["schema_version"],
                limo_cad_sketch::PROJECT_SCHEMA_VERSION
            );
            legacy.as_object_mut().unwrap().remove("print_intent");
            fn remove_guards(value: &mut Value) {
                match value {
                    Value::Object(object) => {
                        object.remove("topology_signature");
                        for child in object.values_mut() {
                            remove_guards(child);
                        }
                    }
                    Value::Array(values) => {
                        for child in values {
                            remove_guards(child);
                        }
                    }
                    _ => {}
                }
            }
            remove_guards(&mut legacy["drawings"]);
            for version in 1..=5 {
                legacy["schema_version"] = json!(version);
                let mut migrated = CadServer::new().unwrap();
                migrated
                    .call_tool(
                        "cad_load_project_model",
                        json!({"model_json":legacy.to_string()}),
                    )
                    .unwrap();
                let resaved = migrated.call_tool("cad_project_model", json!({})).unwrap();
                let resaved: Value = serde_json::from_str(resaved.as_str().unwrap()).unwrap();
                assert_eq!(
                    resaved["schema_version"],
                    limo_cad_sketch::PROJECT_SCHEMA_VERSION
                );
                assert_eq!(
                    serde_json::from_value::<limo_cad_sketch::DrawingDocumentDto>(
                        resaved["drawings"].clone()
                    )
                    .unwrap(),
                    serde_json::from_value::<limo_cad_sketch::DrawingDocumentDto>(
                        legacy["drawings"].clone()
                    )
                    .unwrap(),
                    "migration must not certify legacy ordinal references"
                );
                assert!(migrated
                    .call_tool("drawing_export", json!({"sheet_id":1,"format":"svg"}))
                    .unwrap_err()
                    .to_string()
                    .contains("unverified"));
            }
        }
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool("cad_load_project_model", json!({"model_json":model}))
            .unwrap();
        assert_eq!(
            svg["content"],
            restored
                .call_tool("drawing_export", json!({"sheet_id":1,"format":"svg"}))
                .unwrap()["content"]
        );
    }

    #[test]
    fn embedded_preview_enforces_small_script_limits() {
        let too_long = json!({"version":1,"name":"Long preview","steps":
            (0..81).map(|_|json!({"note":"Another step"})).collect::<Vec<_>>()});
        assert!(preview_script(&too_long.to_string())
            .unwrap_err()
            .contains("80 steps"));
        assert!(preview_script(&" ".repeat(2 * 1024 * 1024 + 1))
            .unwrap_err()
            .contains("2 MiB"));
    }

    #[test]
    fn fillet_recipe_preview_retains_distinct_real_kernel_stages() {
        let source = limo_cad_recipes::find("fillet-basics").unwrap().source;
        let result = preview_script(source).unwrap();
        let frames = result["exports"]["preview_frames"].as_array().unwrap();
        assert_eq!(
            frames.len(),
            2,
            "The lesson shows stock and finished roundover"
        );
        assert!(frames
            .iter()
            .all(|frame| frame["scene"]["bodies"].as_array().unwrap().len() == 1));
        assert_ne!(frames[0]["scene"], frames[1]["scene"]);
        assert_eq!(
            result["exports"]["final_model"]["fillets"][0]["radius"].as_f64(),
            Some(2.0)
        );
    }

    #[test]
    fn resumed_playback_gets_a_fresh_bounded_receipt_wait() {
        for (pauses, complete_after) in [(vec![true, false], 3), (vec![false], 2)] {
            let mut waits = 0;
            let mut states = pauses.into_iter();
            let result = await_playback_receipt(
                || {
                    waits += 1;
                    Ok(json!({"status":if waits == complete_after {"applied"} else {"timeout"}, "seq":17}))
                },
                || Ok(json!({"status":"applied","presentation":{"paused":states.next().unwrap_or(false)}})),
            ).unwrap();

            assert_eq!(waits, complete_after);
            assert_eq!(result["status"], "applied");
            assert_eq!(result["seq"], 17);
        }
        let mut waits = 0;
        let result = await_playback_receipt(
            || {
                waits += 1;
                Ok(json!({"status":"timeout","seq":18}))
            },
            || Ok(json!({"status":"applied","presentation":{"paused":false}})),
        )
        .unwrap();
        assert_eq!(waits, 2);
        assert_eq!(result["status"], "timeout");
    }

    #[test]
    fn attached_assembly_reads_are_fresh_without_reconstructing_cached_geometry() {
        let _guard = session::env_lock();
        let id = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-live-assembly-query-{id}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_box_session(&id);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":id}))
            .unwrap();
        let cached = server.manager.assembly_document();
        let cached_model = server.loaded_snapshot_json.clone();
        server.live_snapshot_dirty = true;

        session::write_session(&id, "model.json", "not a model snapshot").unwrap();
        session::write_session(
            &id,
            "heartbeat.json",
            &json!({"updated_ms":session::now_ms(),"generation":2,"interface_version":1})
                .to_string(),
        )
        .unwrap();
        let peer = id.clone();
        let worker = std::thread::spawn(move || {
            let mut manager = SketchManager::new();
            for index in 1..=2 {
                parse_engine_envelope(host::handle(
                    &mut manager,
                    "assembly_create_component",
                    &json!({"name":format!("Live component {index}")}).to_string(),
                ))
                .unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                loop {
                    let entries =
                        std::fs::read_dir(session::session_dir().join(&peer).join("controls"));
                    let request = entries
                        .ok()
                        .into_iter()
                        .flatten()
                        .filter_map(Result::ok)
                        .find(|entry| {
                            entry
                                .file_name()
                                .to_string_lossy()
                                .ends_with(".request.json")
                        });
                    if let Some(entry) = request {
                        let request: Value =
                            serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap())
                                .unwrap();
                        let query = &request["sketch_query"];
                        assert_eq!(query["method"], "assembly_document");
                        let value = parse_engine_envelope(host::handle(
                            &mut manager,
                            "assembly_document",
                            "",
                        ))
                        .unwrap();
                        std::fs::remove_file(entry.path()).unwrap();
                        session::write_session(
                            &peer,
                            &format!("controls/{}.result.json", request["id"].as_str().unwrap()),
                            &json!({"status":"applied","value":value}).to_string(),
                        )
                        .unwrap();
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "No live assembly query received"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
        for count in 1..=2 {
            let result = server.call_tool("cad_interface",json!({"action":"execute","group":"assembly/joints","operation":"assembly_document","arguments":{}})).unwrap();
            assert_eq!(
                result["component_structure"]["definitions"]
                    .as_array()
                    .unwrap()
                    .len(),
                count
            );
            assert_eq!(server.manager.assembly_document(), cached);
            assert_eq!(server.loaded_snapshot_json, cached_model);
            assert!(server.live_snapshot_dirty);
        }
        worker.join().unwrap();
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parametric_reload_and_wire_results_preserve_floating_point_geometry() {
        let mut original = CadServer::new().unwrap();
        original
            .call_tool(
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            )
            .unwrap();
        original.call_tool("sketch_add_circle", json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":23.75,"y":0.},"ctrl_held":true})).unwrap();
        original.call_tool("sketch_finish", json!({})).unwrap();
        original.call_tool("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":38.7},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]})).unwrap();
        let assembly = original.call_tool("assembly_document", json!({})).unwrap();
        let occurrence = assembly["component_structure"]["occurrences"][0]["id"].clone();

        let translation = [-1.1728120758078999e-17_f64, 5.684341886080804e-14, 0.2];
        original.call_tool("assembly_set_occurrence_pose",json!({"occurrence_id":occurrence,"local_pose":{"translation":translation,"rotation":[0.,0.,0.,1.]}})).unwrap();
        let mut before_solution = original.call_tool("assembly_solution", json!({})).unwrap();
        for (axis, expected) in translation.iter().enumerate() {
            let actual = before_solution["instance_body_poses"][0]["translation"][axis]
                .as_f64()
                .unwrap();
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
        let mut before_scene = original.call_tool("solid_scene", json!({})).unwrap();
        let model = original.call_tool("cad_project_model", json!({})).unwrap();
        let mut reopened = CadServer::new().unwrap();
        reopened
            .call_tool("cad_load_project_model", json!({"model_json":model}))
            .unwrap();
        let mut after_scene = reopened.call_tool("solid_scene", json!({})).unwrap();
        let mut after_solution = reopened.call_tool("assembly_solution", json!({})).unwrap();
        for value in [
            &mut before_scene,
            &mut after_scene,
            &mut before_solution,
            &mut after_solution,
        ] {
            value.as_object_mut().unwrap().remove("_disclosure");
        }
        assert_eq!(before_scene, after_scene);
        assert_eq!(before_solution, after_solution);
        assert_eq!(
            original.manager.export_project_model().unwrap(),
            reopened.manager.export_project_model().unwrap()
        );
    }

    #[test]
    fn command_script_replays_parametric_model_and_refuses_existing_work() {
        let (original, _) = mcp_box();
        let steps: Vec<Value> = original
            .tool_trace
            .iter()
            .map(|call| {
                json!({"call":{
                    "group":interface::group_for(call["name"].as_str().unwrap()),
                    "operation":call["name"],"arguments":call["arguments"]
                }})
            })
            .collect();
        let source = json!({"version":1,"name":"Parametric block","steps":steps}).to_string();
        let mut first = CadServer::new().unwrap();
        let mut second = CadServer::new().unwrap();
        for server in [&mut first, &mut second] {
            let result = server
                .call_tool("cad_interface", json!({"action":"script","source":source}))
                .unwrap();
            assert_eq!(result["steps_completed"], steps.len());
            assert_eq!(server.manager.solid_scene().bodies.len(), 1);
            assert!(server.manager.solid_scene().errors.is_empty());
        }
        let before = first.manager.export_project_model().unwrap();
        assert_eq!(before, second.manager.export_project_model().unwrap());
        let error = first
            .call_tool("cad_interface", json!({"action":"script","source":source}))
            .unwrap_err();
        assert!(error.contains("blank"));
        assert_eq!(before, first.manager.export_project_model().unwrap());
    }

    #[test]
    fn command_script_preserves_a_cam_only_document() {
        let mut server = CadServer::new().unwrap();
        let mut cam = server.manager.cam_document();
        cam.units = serde_json::from_value(json!("inches")).unwrap();
        server.manager.set_cam_document(cam).unwrap();
        let before = server.manager.export_project_model().unwrap();
        let source = json!({"version":1,"name":"Blank only","steps":[
            {"call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":"Unexpected change"}}}
        ]}).to_string();
        let error = server
            .call_tool("cad_interface", json!({"action":"script","source":source}))
            .unwrap_err();
        assert!(error.contains("blank"), "{error}");
        assert_eq!(before, server.manager.export_project_model().unwrap());
    }

    #[test]
    fn command_script_stops_before_later_commands_after_operation_error() {
        let mut server = CadServer::new().unwrap();
        let source = json!({"version":1,"name":"Stop on error","steps":[
            {"id":"first","call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":"Before failure"}}},
            {"id":"bad","call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":""}}},
            {"id":"never","call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":"Must not run"}}}
        ]}).to_string();
        let error = server
            .call_tool("cad_interface", json!({"action":"script","source":source}))
            .unwrap_err();
        assert!(error.contains("bad"));
        assert_eq!(server.manager.document_dto().name, "Before failure");
        assert!(!server.script_running);
    }

    #[test]
    fn replacement_receipt_follows_only_its_new_publisher_and_stops_scripts() {
        let _guard = session::env_lock();
        let original = session::test_session_uuid();
        let replacement = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-replacement-receipt-{original}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (_, original_model) = write_box_session(&original);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":original}))
            .unwrap();
        let receipt = json!({"seq":1,"name":"cad_load_project_model","base_generation":50,
            "project_replaced":true,"previous_session_id":original,
            "active_session_id":replacement,"document_id":"same-native-tab"});
        session::write_session(&original, "inbox/applied/1.json", &receipt.to_string()).unwrap();
        session::write_closed_tombstone(&original).unwrap();
        let waiting = server
            .await_inbox_apply(&json!({"seq":1,"timeout_ms":0}))
            .unwrap();
        assert_eq!(
            waiting["status"], "timeout",
            "replacement must await its own first publication"
        );
        assert_eq!(waiting["active_session_id"], replacement);
        assert_eq!(
            server.attached_document_id.as_deref(),
            Some(original.as_str())
        );
        assert_eq!(
            session::await_inbox_apply(&original, 2, 0, 1).unwrap()["status"],
            "closed",
            "another sequence must not borrow the replacement receipt"
        );
        let mut new_model: Value = serde_json::from_str(&original_model).unwrap();
        new_model["document"]["name"] = json!("Replacement only");
        let publisher = replacement.clone();
        std::thread::spawn(move || {
            session::publish_applied_snapshot(&publisher, &new_model.to_string()).unwrap();
        })
        .join()
        .unwrap();
        let completed = server
            .await_inbox_apply(&json!({"seq":1,"timeout_ms":0}))
            .unwrap();
        assert_eq!(completed["status"], "applied");
        assert_eq!(completed["refreshed"], true);
        assert_eq!(completed["attached_session_id"], replacement);
        assert_eq!(server.manager.document_dto().name, "Replacement only");
        assert_eq!(
            session::require_model_json(&original).unwrap(),
            original_model
        );
        session::write_session(
            &replacement,
            "inbox/applied/2.json",
            &json!({"name":"cad_set_document_name","base_generation":0}).to_string(),
        )
        .unwrap();
        let old_wait = server
            .await_inbox_apply(&json!({"session_id":original,"seq":2,"timeout_ms":0}))
            .unwrap();
        assert_eq!(
            old_wait["status"], "closed",
            "receipt IDs are scoped to their original session"
        );
        let current_wait = server
            .await_inbox_apply(&json!({"seq":2,"timeout_ms":0,"refresh":false}))
            .unwrap();
        assert_eq!(
            current_wait["status"], "applied",
            "omitting session retains current-attachment semantics"
        );
        let retained = server
            .await_inbox_apply(&json!({"session_id":original,"seq":1,"timeout_ms":0}))
            .unwrap();
        assert_eq!(retained["status"], "applied");
        assert_eq!(
            retained["refreshed"], false,
            "reading an older receipt cannot change the current snapshot"
        );
        assert_eq!(
            server.attached_document_id.as_deref(),
            Some(replacement.as_str())
        );

        server.attached_document_id = Some(original.clone());
        server.script_running = true;
        let error = server
            .await_inbox_apply(&json!({"seq":1,"timeout_ms":0,"refresh":false}))
            .unwrap_err();
        assert!(error.contains("Active document changed"));
        assert_eq!(
            server.attached_document_id.as_deref(),
            Some(original.as_str())
        );
        server.script_running = false;
        session::write_closed_tombstone(&replacement).unwrap();
        session::write_session(
            &replacement,
            "heartbeat.json",
            &json!({"generation":2}).to_string(),
        )
        .unwrap();
        assert_eq!(
            session::await_inbox_apply(&original, 1, 0, 1).unwrap()["status"],
            "closed"
        );

        let mut wrong = receipt;
        wrong["previous_session_id"] = json!(replacement);
        session::write_session(&original, "inbox/applied/1.json", &wrong.to_string()).unwrap();
        assert!(session::await_inbox_apply(&original, 1, 0, 1)
            .unwrap_err()
            .contains("ownership"));
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deferred_live_snapshot_is_refreshed_before_a_query() {
        let _guard = session::env_lock();
        let id = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-deferred-script-{id}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_box_session(&id);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":id}))
            .unwrap();
        let mut updated =
            serde_json::from_str::<Value>(&session::require_model_json(&id).unwrap()).unwrap();
        updated["document"]["name"] = json!("New authoritative document");
        session::write_session(&id, "model.json", &updated.to_string()).unwrap();
        server.live_snapshot_dirty = true;
        assert_ne!(
            server.manager.document_dto().name,
            "New authoritative document"
        );
        let result = server.call_tool("cad_document", json!({})).unwrap();
        assert_eq!(result["name"], "New authoritative document");
        assert!(!server.live_snapshot_dirty);
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn session_status_observes_deferred_script_edits_without_advancing_loaded_fence() {
        let _guard = session::env_lock();
        let id = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-script-status-{id}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (_, original_model) = write_box_session(&id);
        session::write_session(
            &id,
            "heartbeat.json",
            &json!({"updated_ms":session::now_ms(),"generation":1,
                "published_generation":1,"model_generation":1,"interface_version":1})
            .to_string(),
        )
        .unwrap();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":id}))
            .unwrap();
        server.script_running = true;

        let peer = id.clone();
        let worker = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while session::pending_inbox_seqs(&peer).unwrap().is_empty() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "No live script edit arrived"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            let seq = session::pending_inbox_seqs(&peer).unwrap()[0];
            let applied = session::apply_inbox_op(&peer, |name, arguments| {
                let mut owner = CadServer::new()?;
                owner.call_tool(
                    "cad_load_project_model",
                    json!({
                        "model_json":session::require_model_json(&peer)?,
                    }),
                )?;
                let value = owner.call_tool(name, arguments)?;

                session::write_session(
                    &peer,
                    &format!("inbox/results/{seq}.json"),
                    &value.to_string(),
                )?;
                let model = owner
                    .manager
                    .export_project_model()
                    .map_err(|e| e.to_string())?;
                session::publish_applied_snapshot(&peer, &model)?;
                Ok(value)
            })
            .unwrap();
            assert_eq!(applied.op.name, "cad_set_document_name");
            let controls = session::session_dir().join(&peer).join("controls");
            loop {
                if let Ok(entries) = std::fs::read_dir(&controls) {
                    for entry in entries.flatten() {
                        if !entry
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".request.json")
                        {
                            continue;
                        }
                        let request: Value =
                            serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap())
                                .unwrap();
                        assert_eq!(request["ui"]["command"], "note");
                        session::write_session(
                            &peer,
                            &format!("controls/{}.result.json", request["id"].as_str().unwrap()),
                            &json!({"status":"applied","active_session_id":peer}).to_string(),
                        )
                        .unwrap();
                        std::fs::remove_file(entry.path()).unwrap();
                        return;
                    }
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "No script caption arrived"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let changed = server
            .call_tool("cad_set_document_name", json!({"name":"Live script edit"}))
            .unwrap();
        assert_eq!(changed["name"], "Live script edit");
        let trace_after_edit = server.tool_trace.clone();
        assert_eq!(
            trace_after_edit.last().unwrap()["name"],
            "cad_set_document_name"
        );
        server
            .call_tool(
                "cad_interface",
                json!({"action":"presentation","command":"note","text":"An edit was applied"}),
            )
            .unwrap();
        worker.join().unwrap();
        let updated_model = session::require_model_json(&id).unwrap();
        assert_ne!(updated_model, original_model);

        session::write_session(&id, "model.json", "invalid deferred snapshot").unwrap();
        let status = server
            .call_tool(
                "cad_interface",
                json!({
                    "action":"execute","group":"document/session",
                    "operation":"cad_session_status","arguments":{},
                }),
            )
            .unwrap();
        assert_eq!(status["attached_generation"], 1);
        assert_eq!(status["generation"], 2);
        assert_eq!(status["model_generation"], 2);
        assert_eq!(status["stale"], true);
        assert_eq!(
            server.loaded_snapshot_json.as_deref(),
            Some(original_model.as_str())
        );
        assert_ne!(server.manager.document_dto().name, "Live script edit");
        assert_eq!(server.tool_trace, trace_after_edit);
        assert!(server.live_snapshot_dirty);
        assert!(server.call_tool("cad_document", json!({})).is_err());
        assert_eq!(server.attached_generation, Some(1));
        assert_eq!(server.tool_trace, trace_after_edit);
        assert!(server.live_snapshot_dirty);

        session::write_session(&id, "model.json", &updated_model).unwrap();
        assert!(server.call_tool("cad_document", json!({})).is_err());
        let recovered = server.call_tool("cad_refresh", json!({})).unwrap();
        assert_eq!(recovered["refreshed"], true);
        assert_eq!(recovered["attached_generation"], 2);
        assert_eq!(
            server.call_tool("cad_document", json!({})).unwrap()["name"],
            "Live script edit"
        );
        assert_eq!(server.attached_generation, Some(2));
        assert!(!server.live_snapshot_dirty);
        assert_eq!(
            server.tool_trace,
            vec![json!({"name":"cad_load_project_model",
            "arguments":{"model_json":updated_model}})]
        );
        assert_eq!(
            server.call_tool("cad_session_status", json!({})).unwrap()["stale"],
            false
        );
        assert!(
            server.script_running,
            "Reads do not release the active script guard"
        );
        server.script_running = false;
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn mcp_box() -> (CadServer, Value) {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": -10.0, "y": -10.0},
                    "p2": {"x": 10.0, "y": 10.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        let update = server
            .call_tool(
                "solid_extrude",
                json!({
                    "sketch_name": "Sketch1",
                    "profile_indices": [0],
                    "operation": "new_body",
                    "extent": {"type": "distance", "distance": 10.0},
                    "taper_angle_deg": 0.0,
                    "flip": false,
                    "target_body_ids": []
                }),
            )
            .unwrap();
        (server, update)
    }

    fn decode_pip_3mf(exported: &Value) -> Vec<u8> {
        assert_eq!(exported["format"], "3mf");
        assert_eq!(exported["encoding"], "base64");
        let b64 = exported["bytes_base64"].as_str().expect("base64 payload");
        assert!(b64.len() > 32);
        let bytes = BASE64.decode(b64).expect("valid base64");
        assert!(bytes.len() > 32);
        assert_eq!(&bytes[0..2], b"PK");
        bytes
    }

    fn pip_model_xml(bytes: &[u8]) -> String {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))
            .expect("3MF should be a zip");
        let mut model = archive.by_name("3D/3dmodel.model").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
        xml
    }

    fn assert_pip_objects(xml: &str, names: &[&str]) {
        for name in names {
            assert!(
                xml.contains(&format!(r#"name="{name}""#)),
                "3MF model missing object {name}"
            );
        }
        let object_count = xml.matches(r#"type="model""#).count();
        assert_eq!(object_count, names.len());
    }

    #[test]
    fn tutor_quest_pip_cam_bolt() {
        let (meshes, apps) = limo_cad_export::print_in_place_cam_bolt();
        assert_eq!(meshes.len(), 4);
        assert_eq!(apps.len(), 4);
        assert_eq!(limo_cad_export::CLEAR_MM, 0.4);

        let mut server = CadServer::new().unwrap();
        let exported = server
            .call_tool("demo_export_pip_3mf", json!({"kind": "cam_bolt"}))
            .expect("cam bolt demo export");
        assert_eq!(exported["demo"], "print_in_place_cam_bolt");
        assert_eq!(exported["body_count"], 4);
        assert!((exported["clearance_mm"].as_f64().unwrap() - 0.4).abs() < 1e-6);
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        assert!(
            scene["bodies"].as_array().unwrap().is_empty(),
            "demo_export_pip_3mf must not mutate the document"
        );
        let bytes = decode_pip_3mf(&exported);
        assert!(bytes.len() > 3_000);
        let xml = pip_model_xml(&bytes);
        assert_pip_objects(
            &xml,
            &[
                "PIP Cam Housing",
                "PIP Cam Bolt",
                "PIP Cam Follower",
                "PIP Cam Dial",
            ],
        );
    }

    #[test]
    fn tutor_quest_pip_clip() {
        let (meshes, apps) = limo_cad_export::print_in_place_clip();
        assert_eq!(meshes.len(), 3);
        assert_eq!(apps.len(), 3);

        let mut server = CadServer::new().unwrap();
        let exported = server
            .call_tool("demo_export_pip_3mf", json!({"kind": "clip"}))
            .expect("clip demo export");
        assert_eq!(exported["demo"], "print_in_place_clip");
        assert_eq!(exported["body_count"], 3);
        assert!((exported["clearance_mm"].as_f64().unwrap() - 0.4).abs() < 1e-6);
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        assert!(scene["bodies"].as_array().unwrap().is_empty());
        let bytes = decode_pip_3mf(&exported);
        assert!(bytes.len() > 2_500);
        let xml = pip_model_xml(&bytes);
        assert_pip_objects(
            &xml,
            &["PIP Clip Housing", "PIP Clip Drawer", "PIP Clip Latch"],
        );
    }

    #[test]
    fn tutor_quest_pip_slicer_variants() {
        let mut server = CadServer::new().unwrap();
        let cases: &[(&str, Option<&str>)] = &[
            ("bambu_studio", None),
            ("orca_slicer", None),
            ("prusa_slicer", Some("Metadata/Slic3r_PE.config")),
            ("cura", Some("Metadata/cura_materials.json")),
            ("standard", None),
        ];
        for (target, extra_file) in cases {
            let exported = server
                .call_tool(
                    "demo_export_pip_3mf",
                    json!({"kind": "cam_bolt", "slicer_target": target}),
                )
                .unwrap_or_else(|error| panic!("{target}: {error}"));
            assert_eq!(exported["slicer_target"], *target);
            assert_eq!(exported["body_count"], 4);
            assert!((exported["clearance_mm"].as_f64().unwrap() - 0.4).abs() < 1e-6);
            let bytes = decode_pip_3mf(&exported);
            let mut archive =
                zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("3MF should be a zip");
            assert!(
                archive.by_name("3D/3dmodel.model").is_ok(),
                "{target} 3MF missing 3D/3dmodel.model"
            );
            if let Some(path) = extra_file {
                assert!(archive.by_name(path).is_ok(), "{target} 3MF missing {path}");
            } else {
                assert!(archive.by_name("Metadata/project_settings.config").is_err());
                assert!(archive.by_name("Metadata/Slic3r_PE.config").is_err());
                assert!(archive.by_name("Metadata/cura_materials.json").is_err());
                let mut model = archive.by_name("3D/3dmodel.model").unwrap();
                let mut xml = String::new();
                std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
                assert!(
                    xml.contains(r#"<metadata name="Application">Limo CAD</metadata>"#),
                    "{target} Application metadata must be exact: {xml}"
                );
                assert!(
                    !xml.contains("BambuStudio"),
                    "{target} must not impersonate Bambu"
                );
                assert!(
                    !xml.contains("OrcaSlicer"),
                    "{target} must not impersonate Orca"
                );
            }
        }
    }

    #[test]
    fn tool_registry_is_granular_and_protocol_lists_revolve() {
        let catalog = full_tool_catalog();
        let all_tools = catalog.as_array().unwrap();
        let mut names = std::collections::HashSet::new();
        for tool in all_tools {
            let name = tool["name"].as_str().unwrap();
            assert!(names.insert(name), "duplicate tool name: {name}");
            let schema = &tool["inputSchema"];
            assert_eq!(schema["type"], "object", "{name}");
            if let Some(required) = schema["required"].as_array() {
                for field in required {
                    assert!(
                        schema["properties"].get(field.as_str().unwrap()).is_some(),
                        "{name}: required field lacks a schema"
                    );
                }
            }
        }
        let mut server = CadServer::new().unwrap();
        let listed = tool_list_result(&mut server.disclosure);
        let tools = listed["tools"].as_array().unwrap();
        assert!(tools.len() < all_tools.len());
        assert!(tools.iter().any(|tool| tool["name"] == "cad_document"));
        assert!(tools.iter().any(|tool| tool["name"] == "cad_get_focus"));

        let initialized = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2025-06-18" }
            }),
        )
        .pop()
        .unwrap();
        assert_eq!(initialized["result"]["protocolVersion"], LATEST_PROTOCOL);
        assert_eq!(
            initialized["result"]["capabilities"]["tools"]["listChanged"],
            true
        );
    }

    #[test]
    fn knowledge_resources_are_advertised_readable_and_leave_the_document_unchanged() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_circle",
                json!({
                    "mode": "center_diameter", "p1": {"x": 0.0, "y": 0.0},
                    "p2": {"x": 10.0, "y": 0.0}, "ctrl_held": true
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        let before = server.manager.export_project_model().unwrap();
        let initialized = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize"
            }),
        );
        assert_eq!(
            initialized[0]["result"]["capabilities"]["resources"],
            json!({"subscribe": false, "listChanged": false})
        );
        let listed = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "resources/list"
            }),
        );
        let resources = listed[0]["result"]["resources"].as_array().unwrap();
        assert!(resources
            .iter()
            .any(|resource| resource["uri"] == "limo-cad://knowledge/index.md"));
        for name in ["gears", "additive-workholding"] {
            let uri = format!("limo-cad://knowledge/concepts/{name}.md");
            let resource = resources
                .iter()
                .find(|resource| resource["uri"] == uri)
                .unwrap();
            assert!(!resource["description"].as_str().unwrap().is_empty());
        }
        let mut seen = std::collections::HashSet::new();
        for resource in resources {
            let uri = resource["uri"].as_str().unwrap();
            assert!(seen.insert(uri), "duplicate resource: {uri}");
            assert!(!resource["title"].as_str().unwrap().is_empty());
            assert_eq!(resource["mimeType"], "text/markdown");
            let reply = handle_message(
                &mut server,
                json!({
                    "jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": {"uri": uri}
                }),
            );
            assert_eq!(reply[0]["id"], 3);
            let contents = reply[0]["result"]["contents"].as_array().unwrap();
            assert_eq!(contents.len(), 1);
            assert_eq!(contents[0]["uri"], uri);
            assert_eq!(contents[0]["mimeType"], "text/markdown");
            let text = contents[0]["text"].as_str().unwrap();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../knowledge")
                .join(resource["name"].as_str().unwrap());
            assert_eq!(text, std::fs::read_to_string(path).unwrap());
            assert_eq!(resource["size"].as_u64(), Some(text.len() as u64));
        }
        assert_eq!(server.manager.export_project_model().unwrap(), before);
    }

    #[test]
    fn knowledge_resources_reject_invalid_params_and_unlisted_uris() {
        let mut server = CadServer::new().unwrap();
        for uri in [
            "limo-cad://knowledge/missing.md",
            "limo-cad://knowledge/../README.md",
            "limo-cad://knowledge/concepts/%2e%2e/index.md",
            "file:///etc/passwd",
            "https://example.com/knowledge/index.md",
            "limo-cad://knowledge/INDEX.md",
        ] {
            let reply = handle_message(
                &mut server,
                json!({
                    "jsonrpc": "2.0", "id": 4, "method": "resources/read", "params": {"uri": uri}
                }),
            );
            assert_eq!(reply[0]["error"]["code"], -32002, "{uri}");
            assert!(reply[0].get("result").is_none());
        }
        for params in [json!(null), json!({}), json!({"uri": 12}), json!([])] {
            let reply = handle_message(
                &mut server,
                json!({
                    "jsonrpc": "2.0", "id": 5, "method": "resources/read", "params": params
                }),
            );
            assert_eq!(reply[0]["error"]["code"], -32602);
        }
        for params in [json!({"cursor": "unknown"}), json!(12)] {
            let reply = handle_message(
                &mut server,
                json!({
                    "jsonrpc": "2.0", "id": 6, "method": "resources/list", "params": params
                }),
            );
            assert_eq!(reply[0]["error"]["code"], -32602);
        }
    }

    #[test]
    fn help_search_prompt_is_advertised_listed_and_gettable() {
        let mut server = CadServer::new().unwrap();
        let initialized = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "2025-06-18" }
            }),
        );
        assert_eq!(
            initialized[0]["result"]["capabilities"]["prompts"],
            json!({ "listChanged": false })
        );
        let listed = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "prompts/list"
            }),
        );
        let prompts = listed[0]["result"]["prompts"].as_array().unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["name"], "help_search");
        assert_eq!(prompts[0]["arguments"][0]["name"], "query");
        assert_eq!(prompts[0]["arguments"][0]["required"], false);

        let got = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "prompts/get",
                "params": {
                    "name": "help_search",
                    "arguments": { "query": "clearance fit" }
                }
            }),
        );
        assert_eq!(got[0]["id"], 3);
        let text = got[0]["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("cad_help"), "{text}");
        assert!(text.contains("Search for: clearance fit"), "{text}");
        assert_eq!(got[0]["result"]["messages"][0]["role"], "user");

        let unknown = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "prompts/get",
                "params": { "name": "validate-before-show" }
            }),
        );
        assert_eq!(unknown[0]["error"]["code"], -32602);
    }

    #[test]
    fn cad_help_search_and_get_clearance_fit() {
        assert_eq!(interface::group_for("cad_help"), Some("document/session"));
        let mut server = CadServer::new().expect("server");
        let search = server
            .call_tool(
                "cad_help",
                json!({"action": "search", "query": "clearance fit", "limit": 5}),
            )
            .expect("search");
        assert_eq!(search["action"], "search");
        let hits = search["hits"].as_array().expect("hits");
        assert!(!hits.is_empty());
        assert!(hits
            .iter()
            .any(|h| h["id"].as_str().unwrap_or("").contains("fits")));
        let id = hits[0]["id"].as_str().unwrap();
        let got = server
            .call_tool("cad_help", json!({"action": "get", "id": id}))
            .expect("get");
        assert_eq!(got["id"], id);
        assert!(got["body"].as_str().unwrap().len() > 20);
        let err = server
            .call_tool("cad_help", json!({"action": "get", "id": "../etc/passwd"}))
            .expect_err("path get must fail");
        assert!(
            err.contains("id")
                || err.contains("invalid")
                || err.contains("path")
                || err.contains("allowlist")
                || err.contains("unknown"),
            "{err}"
        );

        let via_interface = server
            .call_tool(
                "cad_interface",
                json!({
                    "action": "execute",
                    "group": "document/session",
                    "operation": "cad_help",
                    "arguments": {
                        "action": "search",
                        "query": "clearance fit",
                        "limit": 5
                    }
                }),
            )
            .expect("cad_interface cad_help search");
        assert_eq!(via_interface["action"], "search");
        let iface_hits = via_interface["hits"].as_array().expect("iface hits");
        assert!(!iface_hits.is_empty());
        assert!(iface_hits
            .iter()
            .any(|h| h["id"].as_str().unwrap_or("").contains("fits")));
        let iface_id = iface_hits[0]["id"].as_str().unwrap();
        let iface_got = server
            .call_tool(
                "cad_interface",
                json!({
                    "action": "execute",
                    "group": "document/session",
                    "operation": "cad_help",
                    "arguments": {"action": "get", "id": iface_id}
                }),
            )
            .expect("cad_interface cad_help get");
        assert_eq!(iface_got["id"], iface_id);
        assert!(iface_got["body"].as_str().unwrap().len() > 20);
    }

    #[test]
    fn desktop_cad_help_works_before_selection_and_after_detach() {
        let mut server = CadServer::new().unwrap();
        server.desktop_binding = Some(DesktopBinding {
            process_id: u32::MAX,
            initial_selection_pending: true,
        });
        let before = server.manager.export_project_model().unwrap();
        let help_group = interface::group_for("cad_help").unwrap();

        for detached in [false, true] {
            if detached {
                server.call_tool("cad_detach", json!({})).unwrap();
            }
            for operation in ["material_catalog", "printer_catalog"] {
                let direct = server.call_tool(operation, json!({})).unwrap();
                let grouped = server
                    .call_tool(
                        "cad_interface",
                        json!({
                            "action":"execute", "group":interface::group_for(operation).unwrap(),
                            "operation":operation, "arguments":{}
                        }),
                    )
                    .unwrap();
                assert_eq!(grouped, direct);
            }
            for arguments in [
                json!({"action":"search", "query":"clearance fit"}),
                json!({"action":"get", "id":"machine-design.concepts.fits-clearances"}),
                json!({"action":"topics"}),
            ] {
                let direct = server.call_tool("cad_help", arguments.clone()).unwrap();
                let grouped = server
                    .call_tool(
                        "cad_interface",
                        json!({
                            "action":"execute", "group":help_group, "operation":"cad_help",
                            "arguments":arguments
                        }),
                    )
                    .unwrap_or_else(|error| {
                        panic!("grouped help failed (detached={detached}): {error}")
                    });
                assert_eq!(grouped, direct);
            }

            for arguments in [
                json!({"action":"get", "id":"../etc/passwd"}),
                json!({"action":"search"}),
                json!({"action":"unknown"}),
            ] {
                let direct = server
                    .call_tool("cad_help", arguments.clone())
                    .expect_err("invalid help arguments must fail");
                let grouped = server
                    .call_tool(
                        "cad_interface",
                        json!({
                            "action":"execute", "group":help_group, "operation":"cad_help",
                            "arguments":arguments
                        }),
                    )
                    .expect_err("grouped help must preserve argument validation");
                assert_eq!(grouped, direct);
            }

            for operation in ["cad_document", "sketch_begin"] {
                let direct = server
                    .call_tool(operation, json!({}))
                    .expect_err("document operations still need a selected document");
                let grouped = server
                    .call_tool(
                        "cad_interface",
                        json!({
                            "action":"execute", "group":interface::group_for(operation).unwrap(),
                            "operation":operation, "arguments":{}
                        }),
                    )
                    .expect_err("grouped document operations still need a selected document");
                let expected = if detached {
                    "no selected document"
                } else {
                    "desktop_not_ready"
                };
                assert!(direct.contains(expected), "{direct}");
                assert!(grouped.contains(expected), "{grouped}");
            }

            assert!(server.attached_document_id.is_none());
            assert_eq!(
                server
                    .desktop_binding
                    .as_ref()
                    .unwrap()
                    .initial_selection_pending,
                !detached
            );
            assert!(server.tool_trace.is_empty());
            assert_eq!(server.manager.export_project_model().unwrap(), before);
        }
    }

    #[test]
    fn dynamic_disclosure_lists_active_and_soft_tools() {
        DisclosureState::set_clock_for_test(0);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "cad_set_focus",
                json!({"focus": "sketch", "explicit": true}),
            )
            .unwrap();
        let mut listed = tool_list_result(&mut server.disclosure);
        let names: Vec<_> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(names.iter().any(|name| name.starts_with("sketch_")));
        assert!(!names.contains(&"solid_extrude"));

        server
            .call_tool("cad_set_focus", json!({"focus": "solid", "explicit": true}))
            .unwrap();
        listed = tool_list_result(&mut server.disclosure);
        let names: Vec<_> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(names.contains(&"solid_extrude"));
    }

    #[test]
    fn soft_hidden_tools_remain_callable() {
        DisclosureState::set_clock_for_test(0);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "cad_set_focus",
                json!({"focus": "document", "explicit": true}),
            )
            .unwrap();
        DisclosureState::advance_for_test(
            disclosure::SOFT_TTL_MS + disclosure::FOCUS_THROTTLE_MS + 1,
        );
        server.disclosure.tick_soft_expiry();
        let listed = tool_list_result(&mut server.disclosure);
        let names: Vec<_> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(!names.contains(&"sketch_begin"));
        let result = server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();

        assert_eq!(result["_disclosure"]["state"], "soft");
        let listed_after = tool_list_result(&mut server.disclosure);
        assert!(listed_after["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "sketch_begin"));
    }

    #[test]
    fn full_static_lists_entire_registry() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "cad_set_tool_disclosure_mode",
                json!({"mode": "full_static"}),
            )
            .unwrap();
        let listed = tool_list_result(&mut server.disclosure);
        let catalog = full_tool_catalog();
        assert_eq!(
            listed["tools"].as_array().unwrap().len(),
            catalog.as_array().unwrap().len()
        );
    }

    #[test]
    fn every_focus_pack_lists_representative_tools() {
        let expectations: &[(&str, &str)] = &[
            ("document", "cad_project_model"),
            ("sketch", "sketch_begin"),
            ("solid", "solid_extrude"),
            ("modify", "solid_fillet"),
            ("body_ops", "solid_shell"),
            ("datums", "construction_plane_offset"),
            ("history", "solid_delete_feature"),
            ("inspect", "solid_scene"),
            ("print", "solid_export_3mf"),
        ];
        for (focus, tool_name) in expectations {
            let mut server = CadServer::new().unwrap();
            server
                .call_tool("cad_set_focus", json!({ "focus": focus, "explicit": true }))
                .unwrap();
            let listed = tool_list_result(&mut server.disclosure);
            let names: Vec<_> = listed["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| tool["name"].as_str())
                .collect();
            assert!(
                names.iter().any(|name| name == tool_name),
                "focus '{focus}' should advertise '{tool_name}'"
            );
            assert!(
                names.contains(&"cad_get_focus"),
                "spine control tools must remain advertised under '{focus}'"
            );
        }
    }

    #[test]
    fn focus_change_emits_list_changed_without_later_rpc() {
        DisclosureState::set_clock_for_test(0);
        let mut server = CadServer::new().unwrap();
        let responses = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "cad_set_focus",
                    "arguments": {"focus": "sketch", "explicit": true}
                }
            }),
        );
        assert!(
            responses.iter().all(|message| {
                message.get("method").and_then(Value::as_str)
                    != Some("notifications/tools/list_changed")
            }),
            "throttled notify must not flush on the focus-changing response itself"
        );
        assert!(
            server.disclosure.ms_until_wake().is_some(),
            "focus change must schedule a wake for the notify worker"
        );
        DisclosureState::advance_for_test(disclosure::FOCUS_THROTTLE_MS);
        let idle = idle_due_messages(&mut server);
        assert!(
            idle.iter().any(|message| {
                message.get("method").and_then(Value::as_str)
                    == Some("notifications/tools/list_changed")
            }),
            "notify worker must emit list_changed after throttle without a later ping/RPC"
        );
    }

    #[test]
    fn soft_ttl_expiry_emits_list_changed_without_later_rpc() {
        DisclosureState::set_clock_for_test(0);
        let mut server = CadServer::new().unwrap();
        let _ = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "cad_set_focus",
                    "arguments": {"focus": "sketch", "explicit": true}
                }
            }),
        );
        let _ = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "cad_set_focus",
                    "arguments": {"focus": "solid", "explicit": true}
                }
            }),
        );

        DisclosureState::advance_for_test(disclosure::FOCUS_THROTTLE_MS);
        let _ = idle_due_messages(&mut server);

        DisclosureState::advance_for_test(disclosure::SOFT_TTL_MS + 1);
        let after_expiry = idle_due_messages(&mut server);

        if after_expiry.iter().any(|message| {
            message.get("method").and_then(Value::as_str)
                == Some("notifications/tools/list_changed")
        }) {
            return;
        }
        DisclosureState::advance_for_test(disclosure::FOCUS_THROTTLE_MS);
        let idle = idle_due_messages(&mut server);
        assert!(
            idle.iter().any(|message| {
                message.get("method").and_then(Value::as_str)
                    == Some("notifications/tools/list_changed")
            }),
            "soft-TTL expiry must emit list_changed via the notify worker without a later RPC"
        );
    }

    #[test]
    fn read_only_snapshot_attach_refresh_detach() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-attach-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (mut donor, _) = mcp_box();
        let model = donor.call_tool("cad_project_model", json!({})).unwrap();
        let model_json = model
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&model).unwrap());
        session::write_session(&unique, "model.json", &model_json).unwrap();
        session::write_session(&unique, "focus.json", "{\"focus\":\"solid\"}").unwrap();
        session::write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();

        let mut server = CadServer::new().unwrap();

        assert!(server
            .call_tool("cad_attach", json!({"session_id": "My Document"}))
            .is_err());

        let missing = session::test_session_uuid();
        std::fs::create_dir_all(dir.join(&missing)).unwrap();
        assert!(server
            .call_tool("cad_attach", json!({"session_id": missing}))
            .is_err());
        assert!(server.attached_document_id.is_none());

        let listed = server.call_tool("cad_list_sessions", json!({})).unwrap();
        assert_eq!(listed["sessions"][0], unique);
        assert_eq!(listed["session_details"][0]["heartbeat"]["stale"], false);

        let attached = server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        assert_eq!(attached["attached"], true);
        assert_eq!(attached["session_mode"], "read_only_snapshot");
        assert_eq!(attached["writeback"], false);
        assert_eq!(
            server.attached_document_id.as_deref(),
            Some(unique.as_str())
        );
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        assert!(!scene["bodies"].as_array().unwrap().is_empty());

        let refreshed = server.call_tool("cad_refresh", json!({})).unwrap();
        assert_eq!(refreshed["refreshed"], true);
        assert_eq!(refreshed["session_id"], unique);

        let detached = server.call_tool("cad_detach", json!({})).unwrap();
        assert_eq!(detached["detached"], true);
        assert!(server.attached_document_id.is_none());
        assert!(server.call_tool("cad_refresh", json!({})).is_err());

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_targets_window_id_and_document_id() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-attach-mw-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (mut donor, _) = mcp_box();
        let model = donor.call_tool("cad_project_model", json!({})).unwrap();
        let model_json = model
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&model).unwrap());
        session::write_session(&unique, "model.json", &model_json).unwrap();
        session::write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}","window_id":"main","document_id":"tab-a","project_session_id":"tab-a"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();

        let mut server = CadServer::new().unwrap();
        let listed = server.call_tool("cad_list_sessions", json!({})).unwrap();
        assert_eq!(listed["session_details"][0]["window_id"], "main");
        assert_eq!(listed["session_details"][0]["document_id"], "tab-a");
        assert_eq!(listed["windows"][0]["window_id"], "main");

        let by_window = server
            .call_tool("cad_attach", json!({"window_id": "main"}))
            .unwrap();
        assert_eq!(by_window["attached"], true);
        assert_eq!(by_window["session_id"], unique);
        assert_eq!(by_window["window_id"], "main");
        assert_eq!(by_window["document_id"], "tab-a");
        server.call_tool("cad_detach", json!({})).unwrap();

        let by_document = server
            .call_tool("cad_attach", json!({"document_id": "tab-a"}))
            .unwrap();
        assert_eq!(by_document["attached"], true);
        assert_eq!(by_document["session_id"], unique);
        server.call_tool("cad_detach", json!({})).unwrap();

        let headless = session::test_session_uuid();
        session::write_session(&headless, "model.json", &model_json).unwrap();
        session::write_session(
            &headless,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{headless}"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();
        let by_uuid_doc = server
            .call_tool("cad_attach", json!({"document_id": headless}))
            .unwrap();
        assert_eq!(by_uuid_doc["attached"], true);
        assert_eq!(by_uuid_doc["session_id"], headless);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn parse_session_error(error: &str) -> Value {
        serde_json::from_str(error).unwrap_or_else(|_| json!({ "raw": error }))
    }

    fn write_box_session(unique: &str) -> (Value, String) {
        let (mut donor, update) = mcp_box();
        let model = donor.call_tool("cad_project_model", json!({})).unwrap();
        let model_json = model
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&model).unwrap());
        session::write_session(unique, "model.json", &model_json).unwrap();
        session::write_session(
            unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();
        (update, model_json)
    }

    fn solid_mirror_args(body_id: &Value) -> Value {
        json!({
            "body_ids": [body_id],
            "plane": {"type": "origin_plane", "plane": "xy"}
        })
    }

    #[test]
    fn attach_cad_submit_writes_inbox_without_mutating_memory() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-submit-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update, _) = write_box_session(&unique);
        let body_id = update["scene"]["bodies"][0]["id"].clone();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let before = server.call_tool("solid_scene", json!({})).unwrap();
        let before_count = before["bodies"].as_array().unwrap().len();

        let inspect_err = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_scene",
                    "arguments": {},
                    "base_generation": 1
                }),
            )
            .expect_err("inspect tools must not be submitted");
        assert_eq!(
            parse_session_error(&inspect_err)["code"],
            "unsupported_inbox_mutate"
        );

        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_id),
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(submitted["submitted"], true);
        assert_eq!(submitted["seq"], 1);
        assert_eq!(submitted["applied"], false);
        assert_eq!(submitted["writeback"], false);
        assert_eq!(submitted["session_mode"], "ui_owned_apply");
        let inbox = session::read_session_file(&unique, "inbox/1.json").unwrap();
        assert!(inbox.contains("solid_mirror"));

        let after = server.call_tool("solid_scene", json!({})).unwrap();
        assert_eq!(after["bodies"].as_array().unwrap().len(), before_count);
        let project = server.call_tool("cad_project_model", json!({})).unwrap();
        let project_text = project
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| project.to_string());
        assert!(
            !project_text.to_lowercase().contains("mirror"),
            "MCP in-memory model must stay unchanged until cad_refresh: {project_text}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_box_session_with_identity(
        unique: &str,
        window_id: &str,
        document_id: &str,
        process_id: &str,
    ) -> (Value, String) {
        let (update, model_json) = write_box_session(unique);

        session::write_session(
            unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}","window_id":"{window_id}","document_id":"{document_id}","project_session_id":"{document_id}","process_instance_id":"{process_id}"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();
        (update, model_json)
    }

    fn write_two_window_process_lease(
        dir: &std::path::Path,
        process_id: &str,
        session_a: &str,
        session_b: &str,
    ) {
        let processes = dir.join("_ui").join("processes");
        std::fs::create_dir_all(&processes).unwrap();
        std::fs::write(
            processes.join(format!("{process_id}.json")),
            serde_json::to_string_pretty(&json!({
                "process_instance_id": process_id,
                "updated_ms": session::now_ms(),
                "windows": [
                    {
                        "window_id": "main",
                        "active_document_id": "tab-a",
                        "active_session_id": session_a,
                    },
                    {
                        "window_id": "secondary",
                        "active_document_id": "tab-b",
                        "active_session_id": session_b,
                    }
                ],
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn attach_submit_on_window_a_does_not_clobber_window_b() {
        let _guard = session::env_lock();
        let session_a = session::test_session_uuid();
        let session_b = format!(
            "00000000-0000-4000-8000-{:012x}",
            (session::now_ms().wrapping_add(41)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-clobber-{session_a}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update_a, model_a) =
            write_box_session_with_identity(&session_a, "main", "tab-a", "proc-clobber");
        let (_update_b, model_b) =
            write_box_session_with_identity(&session_b, "secondary", "tab-b", "proc-clobber");
        write_two_window_process_lease(&dir, "proc-clobber", &session_a, &session_b);
        let body_id = update_a["scene"]["bodies"][0]["id"].clone();
        let model_b_before = model_b.clone();

        let mut server = CadServer::new().unwrap();
        let attached = server
            .call_tool(
                "cad_attach",
                json!({"window_id": "main", "document_id": "tab-a"}),
            )
            .unwrap();
        assert_eq!(attached["session_id"], session_a);
        assert_eq!(attached["window_id"], "main");

        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_id),
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(submitted["submitted"], true);
        assert_eq!(submitted["seq"], 1);
        assert_eq!(submitted["session_id"], session_a);
        assert_eq!(submitted["window_id"], "main");
        assert_eq!(submitted["document_id"], "tab-a");

        let inbox_a = session::read_session_file(&session_a, "inbox/1.json").unwrap();
        let parsed_a: Value = serde_json::from_str(&inbox_a).unwrap();
        assert_eq!(parsed_a["session_id"], session_a);
        assert_eq!(parsed_a["window_id"], "main");
        assert_eq!(parsed_a["document_id"], "tab-a");
        assert!(
            session::pending_inbox_seqs(&session_b).unwrap().is_empty(),
            "window B inbox must stay empty"
        );
        assert!(
            !dir.join(&session_b).join("inbox").exists()
                || session::pending_inbox_seqs(&session_b).unwrap().is_empty()
        );
        let model_b_after = session::read_session_file(&session_b, "model.json").unwrap();
        assert_eq!(
            model_b_after, model_b_before,
            "window B model.json must be unchanged"
        );
        assert_eq!(
            session::read_session_file(&session_a, "model.json").unwrap(),
            model_a,
            "submit must not write model.json on A either"
        );

        let list = server.call_tool("cad_list_sessions", json!({})).unwrap();
        assert_eq!(list["windows"].as_array().unwrap().len(), 2);
        let window_ids: Vec<_> = list["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["window_id"].as_str().unwrap().to_string())
            .collect();
        assert!(window_ids.contains(&"main".to_string()));
        assert!(window_ids.contains(&"secondary".to_string()));

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switch_attach_to_b_submit_does_not_land_in_a() {
        let _guard = session::env_lock();
        let session_a = session::test_session_uuid();
        let session_b = format!(
            "00000000-0000-4000-8000-{:012x}",
            (session::now_ms().wrapping_add(43)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-switch-{session_a}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update_a, _) =
            write_box_session_with_identity(&session_a, "main", "tab-a", "proc-switch");
        let (update_b, _) =
            write_box_session_with_identity(&session_b, "secondary", "tab-b", "proc-switch");
        write_two_window_process_lease(&dir, "proc-switch", &session_a, &session_b);
        let body_a = update_a["scene"]["bodies"][0]["id"].clone();
        let body_b = update_b["scene"]["bodies"][0]["id"].clone();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"window_id": "main"}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_a),
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(session::pending_inbox_seqs(&session_a).unwrap(), vec![1]);
        let a_pending_after_first = session::pending_inbox_seqs(&session_a).unwrap().len();

        server.call_tool("cad_detach", json!({})).unwrap();
        let attached_b = server
            .call_tool("cad_attach", json!({"window_id": "secondary"}))
            .unwrap();
        assert_eq!(attached_b["session_id"], session_b);

        let submitted_b = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_b),
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(submitted_b["submitted"], true);
        assert_eq!(submitted_b["session_id"], session_b);
        assert_eq!(submitted_b["window_id"], "secondary");
        assert_eq!(session::pending_inbox_seqs(&session_b).unwrap(), vec![1]);
        assert_eq!(
            session::pending_inbox_seqs(&session_a).unwrap().len(),
            a_pending_after_first,
            "A pending seqs must not grow after switch+submit on B"
        );
        let inbox_b = session::read_session_file(&session_b, "inbox/1.json").unwrap();
        let parsed_b: Value = serde_json::from_str(&inbox_b).unwrap();
        assert_eq!(parsed_b["session_id"], session_b);
        assert_eq!(parsed_b["window_id"], "secondary");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_inbox_helper_on_separate_manager_then_refresh_sees_body() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-apply-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update, _) = write_box_session(&unique);
        let body_id = update["scene"]["bodies"][0]["id"].clone();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_id),
                    "base_generation": 1
                }),
            )
            .unwrap();
        let before = server.call_tool("solid_scene", json!({})).unwrap();
        let before_count = before["bodies"].as_array().unwrap().len();

        let applied = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            let result = host.call_tool(name, arguments)?;
            let exported = host.call_tool("cad_project_model", json!({}))?;
            let model_json = exported
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(&exported).unwrap());
            session::publish_applied_snapshot(&unique, &model_json)?;
            Ok(result)
        })
        .expect("apply helper should run host on a separate SketchManager");
        assert_eq!(applied.seq, 1);
        assert_eq!(applied.op.name, "solid_mirror");
        assert!(
            applied.host_result.is_object(),
            "separate host apply should return an engine object"
        );
        assert_eq!(session::read_heartbeat_generation(&unique).unwrap(), 2);
        assert!(session::pending_inbox_seqs(&unique).unwrap().is_empty());

        let still_old = server.call_tool("solid_scene", json!({})).unwrap();
        assert_eq!(still_old["bodies"].as_array().unwrap().len(), before_count);

        server.call_tool("cad_refresh", json!({})).unwrap();
        let refreshed = server.call_tool("solid_scene", json!({})).unwrap();
        let after_count = refreshed["bodies"].as_array().unwrap().len();
        assert!(
            after_count > before_count,
            "cad_refresh must see the applied body (before {before_count}, after {after_count})"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_base_generation_is_generation_conflict_and_does_not_apply() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-stale-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update, _) = write_box_session(&unique);
        let body_id = update["scene"]["bodies"][0]["id"].clone();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let submit_err = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_id),
                    "base_generation": 99
                }),
            )
            .expect_err("stale cad_submit must fail");
        let parsed = parse_session_error(&submit_err);
        assert_eq!(parsed["code"], "generation_conflict");
        assert_eq!(parsed["writeback"], false);
        assert_eq!(parsed["session_mode"], "ui_owned_apply");
        assert!(session::pending_inbox_seqs(&unique).unwrap().is_empty());

        session::write_inbox_op(
            &unique,
            &session::InboxOp::unstamped(
                "solid_mirror".to_string(),
                solid_mirror_args(&body_id),
                99,
            ),
        )
        .unwrap();
        let mut applied = false;
        let apply_err = session::apply_inbox_op(&unique, |_name, _args| {
            applied = true;
            Ok(json!({}))
        })
        .expect_err("stale apply must fail");
        assert!(!applied, "host must not run on generation_conflict");
        let applied_err = parse_session_error(&apply_err);
        assert_eq!(applied_err["code"], "generation_conflict");
        assert!(
            session::pending_inbox_seqs(&unique).unwrap().is_empty(),
            "stale head must dead-letter so later seqs can apply"
        );
        assert!(
            std::path::Path::new(&dir)
                .join(&unique)
                .join("inbox/failed/1.json")
                .exists(),
            "expected inbox/failed/1.json"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_submit_without_attach_fails() {
        let mut server = CadServer::new().unwrap();
        let err = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": {},
                    "base_generation": 1
                }),
            )
            .expect_err("cad_submit without attach must fail");
        let parsed = parse_session_error(&err);
        assert_eq!(parsed["code"], "not_attached");
        assert_eq!(parsed["writeback"], false);
        assert_eq!(parsed["session_mode"], "ui_owned_apply");
    }

    #[test]
    fn cad_await_apply_without_attach_fails() {
        let mut server = CadServer::new().unwrap();
        let err = server
            .call_tool("cad_await_apply", json!({ "seq": 1 }))
            .expect_err("cad_await_apply without attach must fail");
        let parsed = parse_session_error(&err);
        assert_eq!(parsed["code"], "not_attached");
        assert_eq!(parsed["writeback"], false);
        assert_eq!(parsed["session_mode"], "ui_owned_apply");
    }

    #[test]
    fn cad_session_status_headless_is_not_attached_not_error() {
        let mut server = CadServer::new().unwrap();
        let status = server
            .call_tool("cad_session_status", json!({}))
            .expect("headless status must succeed");
        assert_eq!(status["attached"], false);
        assert_eq!(status["code"], "not_attached");
        assert_eq!(status["writeback"], false);
        assert_eq!(status["pending_inbox_count"], 0);
        assert!(status["last_apply_receipt"].is_null());
    }

    #[test]
    fn cad_session_status_reports_stale_pending_and_receipt() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-status-tool-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update, _) = write_box_session(&unique);
        let body_id = update["scene"]["bodies"][0]["id"].clone();

        let mut server = CadServer::new().unwrap();
        let attached = server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        assert_eq!(attached["attached_generation"], 1);
        assert_eq!(server.attached_generation, Some(1));

        let status = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(status["attached"], true);
        assert_eq!(status["session_id"], unique);
        assert_eq!(status["attached_generation"], 1);
        assert_eq!(status["generation"], 1);
        assert_eq!(status["stale"], false);
        assert_eq!(status["pending_inbox_count"], 0);

        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_id),
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(submitted["seq"], 1);

        session::write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":2,"model_generation":2,"session_id":"{unique}","kind":"snapshot","session_mode":"read_only_snapshot"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();

        let stale = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(stale["stale"], true);
        assert_eq!(stale["attached_generation"], 1);
        assert_eq!(stale["generation"], 2);
        assert_eq!(stale["pending_inbox"], json!([1]));
        assert_eq!(stale["pending_inbox_count"], 1);
        assert_eq!(stale["heartbeat_kind"], "snapshot");

        let _ = session::apply_inbox_op(&unique, |_n, _a| Ok(json!({}))).expect_err("stale");
        let after = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(after["pending_inbox_count"], 0);
        assert_eq!(after["last_apply_receipt"]["seq"], 1);
        assert_eq!(after["last_apply_receipt"]["status"], "failed");

        let detached = server.call_tool("cad_detach", json!({})).unwrap();
        assert_eq!(detached["detached"], true);
        assert!(server.attached_generation.is_none());
        let headless = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(headless["attached"], false);
        assert_eq!(headless["code"], "not_attached");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_session_status_engine_revision_attach_reports_model_fence_stale() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir =
            std::env::temp_dir().join(format!("limo-cad-sessions-status-engine-rev-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (_update, _) = write_box_session(&unique);
        session::write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{unique}","kind":"engine_revision","session_mode":"ui_owned_apply"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();

        let mut server = CadServer::new().unwrap();
        let attached = server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        assert_eq!(
            attached["attached_generation"], 1,
            "attach must record model publication generation, not live engine generation"
        );
        assert_eq!(server.attached_generation, Some(1));

        let status = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(status["attached_generation"], 1);
        assert_eq!(status["generation"], 2);
        assert_eq!(status["published_generation"], 1);
        assert_eq!(status["model_generation"], 1);
        assert_eq!(status["heartbeat_kind"], "engine_revision");
        assert_eq!(
            status["stale"], true,
            "loaded model gen 1 must be stale vs live engine generation 2"
        );
        let hint = status["hint"].as_str().unwrap_or("");
        assert!(
            !hint.contains("matches live"),
            "must not claim fresh match: {hint}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_await_apply_refreshes_after_separate_host_publish() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-await-tool-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update, _) = write_box_session(&unique);
        let body_id = update["scene"]["bodies"][0]["id"].clone();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_id),
                    "base_generation": 1
                }),
            )
            .unwrap();
        let seq = submitted["seq"].as_u64().unwrap();
        let before = server.call_tool("solid_scene", json!({})).unwrap();
        let before_count = before["bodies"].as_array().unwrap().len();

        let session_for_worker = unique.clone();
        let mirror_args = solid_mirror_args(&body_id);
        let worker = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            let applied = session::apply_inbox_op(&session_for_worker, |name, arguments| {
                let mut host = CadServer::new()?;
                let model = session::require_model_json(&session_for_worker)?;
                host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
                let result = host.call_tool(name, arguments)?;
                let exported = host.call_tool("cad_project_model", json!({}))?;
                let model_json = exported
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| serde_json::to_string(&exported).unwrap());

                session::write_session(
                    &session_for_worker,
                    "heartbeat.json",
                    &format!(
                        r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{session_for_worker}","kind":"engine_revision","session_mode":"ui_owned_apply"}}"#,
                        session::now_ms()
                    ),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(30));


                session::write_session(
                    &session_for_worker,
                    "heartbeat.json",
                    &format!(
                        r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{session_for_worker}","kind":"heartbeat","session_mode":"read_only_snapshot"}}"#,
                        session::now_ms()
                    ),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(60));
                session::publish_applied_snapshot(&session_for_worker, &model_json)?;
                Ok(result)
            })
            .expect("host apply");
            assert_eq!(applied.op.name, "solid_mirror");
            assert_eq!(applied.op.arguments, mirror_args);
        });

        let awaited = server
            .call_tool(
                "cad_await_apply",
                json!({
                    "seq": seq,
                    "timeout_ms": 3000,
                    "poll_ms": 20,
                    "refresh": true
                }),
            )
            .unwrap();
        worker.join().unwrap();
        assert_eq!(awaited["status"], "applied");
        assert_eq!(awaited["published"], true);
        assert_eq!(awaited["model_published"], true);
        assert_eq!(awaited["active_sketch_published"], false);
        assert_eq!(awaited["refreshed"], true);
        assert_eq!(awaited["timed_out"], false);
        assert_eq!(awaited["writeback"], false);
        assert_eq!(awaited["session_mode"], "ui_owned_apply");

        let status = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(status["attached_generation"], awaited["model_generation"]);
        assert_eq!(status["stale"], false);

        let after = server.call_tool("solid_scene", json!({})).unwrap();
        let after_count = after["bodies"].as_array().unwrap().len();
        assert!(
            after_count > before_count,
            "await+refresh must load applied body (before {before_count}, after {after_count})"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_await_apply_does_not_refresh_active_sketch_only_snapshot() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-await-sketch-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_box_session(&unique);

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let before = server.call_tool("solid_scene", json!({})).unwrap();
        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "sketch_begin",
                    "arguments": {
                        "plane": {"type": "origin_plane", "plane": "xy"}
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let seq = submitted["seq"].as_u64().unwrap();
        session::apply_inbox_op(&unique, |_name, _args| Ok(json!({"ok": true}))).unwrap();
        session::write_session(
            &unique,
            "active-sketch.json",
            r#"{"name":"Sketch1","entities":[]}"#,
        )
        .unwrap();
        session::write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":2,"model_generation":1,"active_sketch_generation":2,"session_id":"{unique}","kind":"snapshot"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();

        let awaited = server
            .call_tool(
                "cad_await_apply",
                json!({"seq": seq, "timeout_ms": 0, "refresh": true}),
            )
            .unwrap();
        assert_eq!(awaited["status"], "applied");
        assert_eq!(awaited["published"], true);
        assert_eq!(awaited["model_published"], false);
        assert_eq!(awaited["active_sketch_published"], true);
        assert_eq!(awaited["snapshot_kind"], "active_sketch");
        assert_eq!(awaited["refreshed"], false);
        assert!(awaited["hint"]
            .as_str()
            .unwrap_or_default()
            .contains("active-sketch"));
        assert_eq!(server.call_tool("solid_scene", json!({})).unwrap(), before);
        let status = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(status["attached_generation"], 1);
        assert_eq!(status["generation"], 2);
        assert_eq!(status["active_sketch_generation"], 2);
        assert_eq!(status["stale"], true);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_await_apply_timeout_probe_while_pending() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-await-probe-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update, _) = write_box_session(&unique);
        let body_id = update["scene"]["bodies"][0]["id"].clone();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_id),
                    "base_generation": 1
                }),
            )
            .unwrap();
        let seq = submitted["seq"].as_u64().unwrap();

        let probe = server
            .call_tool(
                "cad_await_apply",
                json!({ "seq": seq, "timeout_ms": 0, "refresh": false }),
            )
            .unwrap();
        assert_eq!(probe["status"], "pending");
        assert_eq!(probe["timed_out"], false);
        assert_eq!(probe["applied"], false);
        assert_eq!(probe["refreshed"], false);

        let timed = server
            .call_tool(
                "cad_await_apply",
                json!({ "seq": seq, "timeout_ms": 40, "poll_ms": 10, "refresh": false }),
            )
            .unwrap();
        assert_eq!(timed["status"], "timeout");
        assert_eq!(timed["timed_out"], true);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn headless_goldens_still_mutate_without_attach() {
        let (mut server, _) = mcp_box();
        assert!(server.attached_document_id.is_none());
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        let body_id = scene["bodies"][0]["id"].clone();
        server
            .call_tool("solid_mirror", solid_mirror_args(&body_id))
            .expect("headless CadServer with no attach must still mutate");
        let after = server.call_tool("solid_scene", json!({})).unwrap();
        assert!(after["bodies"].as_array().unwrap().len() > 1);
    }

    #[test]
    fn solid_export_3mf_returns_base64_payload() {
        let (mut server, _) = mcp_box();
        let exported = server
            .call_tool("solid_export_3mf", json!({"slicer_target": "bambu_studio"}))
            .expect("3MF export should succeed for a simple box");
        assert_eq!(exported["format"], "3mf");
        assert_eq!(exported["encoding"], "base64");
        let b64 = exported["bytes_base64"].as_str().expect("base64 payload");
        assert!(b64.len() > 32);
        let bytes = BASE64.decode(b64).expect("valid base64");
        assert!(bytes.len() > 32);

        assert_eq!(&bytes[0..2], b"PK");
    }

    fn parse_3mf_model_mesh(xml: &str) -> limo_cad_export::TriangleMesh {
        use limo_cad_core::BodyId;
        let mut positions = Vec::new();
        for line in xml.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("<vertex x=\"") {
                let parts: Vec<&str> = rest.split('"').collect();
                if parts.len() >= 6 {
                    let x: f64 = parts[0].parse().unwrap();
                    let y: f64 = parts[2].parse().unwrap();
                    let z: f64 = parts[4].parse().unwrap();
                    positions.extend_from_slice(&[x, y, z]);
                }
            }
        }
        let mut indices = Vec::new();
        for line in xml.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("<triangle v1=\"") {
                let parts: Vec<&str> = rest.split('"').collect();
                if parts.len() >= 6 {
                    indices.push(parts[0].parse().unwrap());
                    indices.push(parts[2].parse().unwrap());
                    indices.push(parts[4].parse().unwrap());
                }
            }
        }
        limo_cad_export::TriangleMesh {
            body_id: BodyId(1),
            name: "exported".into(),
            positions,
            indices,
        }
    }

    #[test]
    fn definition_mesh_export_selects_one_unplaced_native_part() {
        let (mut server, initial) = mcp_box();
        let body = initial["scene"]["bodies"][0]["id"].clone();
        let document = server.call_tool("assembly_document", json!({})).unwrap();
        let occurrence = document["component_structure"]["occurrences"][0].clone();
        server.call_tool("assembly_set_occurrence_pose", json!({"occurrence_id":occurrence["id"],"local_pose":{"translation":[0.,0.,70.],"rotation":[0.,0.,0.,1.]}})).unwrap();
        server.call_tool("assembly_create_occurrence", json!({"component_id":occurrence["component_id"],"name":"Repeated print","local_pose":{"translation":[0.,0.,170.],"rotation":[0.,0.,0.,1.]}})).unwrap();
        let before = server.manager.export_project_model().unwrap();
        let assemble = server
            .call_tool(
                "solid_export_3mf",
                json!({"body_ids":[body],"slicer_target":"standard"}),
            )
            .unwrap();
        let xml = pip_model_xml(
            &BASE64
                .decode(assemble["bytes_base64"].as_str().unwrap())
                .unwrap(),
        );
        assert_eq!(xml.matches("<mesh>").count(), 1);
        assert_eq!(
            limo_cad_export::test_reader::read_build(&xml)
                .unwrap()
                .len(),
            2
        );
        let definition = server
            .call_tool(
                "solid_export_3mf",
                json!({"body_ids":[body],"scope":"definition","slicer_target":"standard"}),
            )
            .unwrap();
        let xml = pip_model_xml(
            &BASE64
                .decode(definition["bytes_base64"].as_str().unwrap())
                .unwrap(),
        );
        assert_eq!(xml.matches("<mesh>").count(), 1);
        let mesh = parse_3mf_model_mesh(&xml);
        limo_cad_export::validate_3mf_model_mesh(&mesh).unwrap();
        assert!(mesh
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .all(|point| (0.0..=10.0).contains(&point[2])));
        let stl = server
            .call_tool(
                "solid_export_stl",
                json!({"body_ids":[body],"scope":"definition"}),
            )
            .unwrap();
        let bytes = BASE64
            .decode(stl["bytes_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 12);
        assert_eq!(server.manager.export_project_model().unwrap(), before);
    }

    #[test]
    fn mesh_and_step_export_snapshots_reject_mcp_open_and_assembly_edits() {
        let (mut server, initial) = mcp_box();
        let body = initial["scene"]["bodies"][0]["id"].clone();
        let expected = server.manager.export_project_model().unwrap();
        let request = json!({"body_ids":[body],"expected_model_json":expected,"scope":"definition","slicer_target":"standard"});
        let original = server
            .call_tool("solid_export_3mf", request.clone())
            .unwrap();
        let step_request = json!({"body_ids":[body],"expected_model_json":expected,
            "occurrences":[{"occurrence_id":1,"component_id":1,"body_id":body,
                "translation":[42.,0.,0.],"rotation":[0.,0.,0.,1.],"name":"Selected occurrence"}]});
        let specs = tool_specs();
        schema_accepts(
            &specs
                .iter()
                .find(|spec| spec.name == "solid_export_step")
                .unwrap()
                .input_schema,
            &step_request,
        )
        .unwrap();
        let step = server
            .call_tool("solid_export_step", step_request.clone())
            .unwrap();
        let step_bytes = BASE64
            .decode(step["bytes_base64"].as_str().unwrap())
            .unwrap();
        assert!(String::from_utf8(step_bytes)
            .unwrap()
            .contains("MANIFOLD_SOLID_BREP"));
        let (mut replacement, _) = mcp_box();
        let document = replacement
            .call_tool("assembly_document", json!({}))
            .unwrap();
        let occurrence = &document["component_structure"]["occurrences"][0]["id"];
        replacement.call_tool("assembly_set_occurrence_pose", json!({"occurrence_id":occurrence,"local_pose":{"translation":[0.,0.,90.],"rotation":[0.,0.,0.,1.]}})).unwrap();
        let replacement_model = replacement.manager.export_project_model().unwrap();

        server
            .call_tool(
                "cad_load_project_model",
                json!({"model_json":replacement_model}),
            )
            .unwrap();
        for operation in ["solid_export_stl", "solid_export_3mf"] {
            assert!(server
                .call_tool(operation, request.clone())
                .unwrap_err()
                .contains("document changed"));
        }
        assert!(server
            .call_tool("solid_export_step", step_request.clone())
            .unwrap_err()
            .contains("document changed"));
        assert_eq!(
            server.manager.export_project_model().unwrap(),
            replacement_model
        );

        assert!(server
            .call_tool("solid_export_stl", json!({"body_ids":[body]}))
            .is_ok());
        assert!(server
            .call_tool("solid_export_step", json!({"body_ids":[body]}))
            .is_ok());
        server
            .call_tool("cad_load_project_model", json!({"model_json":expected}))
            .unwrap();
        assert_eq!(
            server.call_tool("solid_export_3mf", request).unwrap()["bytes_base64"],
            original["bytes_base64"]
        );
        assert!(server.call_tool("solid_export_step", step_request).is_ok());
    }

    #[test]
    fn assembly_mesh_export_omits_unused_definitions_after_model_reload() {
        let (mut server, initial) = mcp_box();
        let placed_body = initial["scene"]["bodies"][0]["id"].clone();
        let updated = extrude_offset_box(&mut server, "Sketch2", 30., 40.);
        let unused_body = updated["scene"]["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|body| body["id"] != placed_body)
            .unwrap()["id"]
            .clone();
        let component = server.call_tool("assembly_create_component", json!({
            "name":"Reusable but unplaced", "body_ids":[unused_body], "absorb_promoted_bodies":true
        })).unwrap();
        let mut model: Value =
            serde_json::from_str(&server.manager.export_project_model().unwrap()).unwrap();
        model["assembly"]["component_structure"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .retain(|occurrence| occurrence["component_id"] != component["id"]);
        server
            .call_tool(
                "cad_load_project_model",
                json!({"model_json":model.to_string()}),
            )
            .unwrap();
        let solution = server.call_tool("assembly_solution", json!({})).unwrap();
        assert_eq!(solution["solved"], true);
        assert_eq!(solution["instance_body_poses"].as_array().unwrap().len(), 1);
        assert_eq!(server.manager.solid_scene().bodies.len(), 2);
        let before = server.manager.export_project_model().unwrap();
        let exported = server
            .call_tool("solid_export_3mf", json!({"slicer_target":"standard"}))
            .unwrap();
        let bytes = BASE64
            .decode(exported["bytes_base64"].as_str().unwrap())
            .unwrap();
        let xml = pip_model_xml(&bytes);
        assert_eq!(xml.matches("<mesh>").count(), 1);
        let mesh = parse_3mf_model_mesh(&xml);
        limo_cad_export::validate_3mf_model_mesh(&mesh).unwrap();
        assert!(mesh
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .all(|point| point[0] <= 10.));
        let exported = server.call_tool("solid_export_stl", json!({})).unwrap();
        let bytes = BASE64
            .decode(exported["bytes_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 12);
        assert_eq!(server.manager.export_project_model().unwrap(), before);

        model["assembly"]["component_structure"]["occurrences"] = json!([]);
        for definition in model["assembly"]["component_structure"]["definitions"]
            .as_array_mut()
            .unwrap()
        {
            definition["promoted"] = json!(false);
        }
        server
            .call_tool(
                "cad_load_project_model",
                json!({"model_json":model.to_string()}),
            )
            .unwrap();
        assert!(server
            .manager
            .assembly_solution()
            .instance_body_poses
            .is_empty());
        assert!(server
            .call_tool("solid_export_stl", json!({}))
            .unwrap_err()
            .contains("no active bodies"));
        assert!(server
            .call_tool("solid_export_3mf", json!({"slicer_target":"standard"}))
            .is_err());
    }

    #[test]
    fn assembly_drawing_projects_rotated_instances_and_shared_hidden_line_occlusion() {
        let (mut server, _) = mcp_box();
        let document = server.call_tool("assembly_document", json!({})).unwrap();
        let original = document["component_structure"]["occurrences"][0]["id"].clone();
        let component = document["component_structure"]["occurrences"][0]["component_id"].clone();
        let added = server
            .call_tool(
                "assembly_create_occurrence",
                json!({"component_id":component,"name":"Rotated copy"}),
            )
            .unwrap();
        let angle = std::f64::consts::FRAC_PI_8;
        server.call_tool("assembly_set_occurrence_pose",json!({"occurrence_id":added["id"],"local_pose":{"translation":[100.,0.,0.],"rotation":[0.,0.,angle.sin(),angle.cos()]}})).unwrap();
        let request = json!({"scope":"assembly","direction":[0.,0.,1.],"up":[0.,1.,0.],"include_hidden":true});
        let before = server.call_tool("cad_project_model", json!({})).unwrap();
        let projection = server
            .call_tool("drawing_projection", request.clone())
            .unwrap();
        assert!((projection["bounds"][0].as_f64().unwrap() + 10.).abs() < 1e-6);
        assert!(
            (projection["bounds"][2].as_f64().unwrap() - (100. + 10. * 2_f64.sqrt())).abs() < 1e-6
        );
        let anchors = projection["anchors"].as_array().unwrap();
        assert!(anchors.iter().any(|a| a["occurrence_id"] == original));
        let placed = anchors
            .iter()
            .find(|a| a["occurrence_id"] == added["id"])
            .unwrap();
        let reference:limo_cad_sketch::DrawingTopologyAnchorRefDto=serde_json::from_value(json!({"topology_signature":projection["topology_signatures"][placed["body_id"].to_string()],"occurrence_id":placed["occurrence_id"],"body_id":placed["body_id"],"edge_id":placed["edge_id"],"edge_key":placed["edge_key"],"endpoint":placed["endpoint"],"fallback_point":[999.,999.,999.]})).unwrap();
        let resolved = limo_cad_occt::resolve_drawing_anchor(
            &server.manager.solid_scene(),
            &server.manager.assembly_document(),
            &reference,
        )
        .unwrap();
        let expected: [f64; 3] = serde_json::from_value(placed["model_point"].clone()).unwrap();
        for (actual, expected) in resolved.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-8);
        }
        assert_eq!(
            server.call_tool("cad_project_model", json!({})).unwrap(),
            before
        );
        let mut definition = request.clone();
        definition["scope"] = json!("definition");
        let definition = server.call_tool("drawing_projection", definition).unwrap();
        assert!((definition["bounds"][2].as_f64().unwrap() - 10.).abs() < 1e-6);
        assert!(definition["anchors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["occurrence_id"].is_null()));
        let mut selected = request.clone();
        selected["occurrence_ids"] = json!([added["id"]]);
        let selected = server.call_tool("drawing_projection", selected).unwrap();
        assert!(selected["anchors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["occurrence_id"] == added["id"]));
        assert!(server.call_tool("drawing_projection",json!({"scope":"assembly","occurrence_ids":[9999],"direction":[0.,0.,1.],"up":[0.,1.,0.]})).is_err());

        server.call_tool("assembly_set_occurrence_pose",json!({"occurrence_id":added["id"],"local_pose":{"translation":[5.,0.,30.],"rotation":[0.,0.,0.,1.]}})).unwrap();
        let occluded = server.call_tool("drawing_projection", request).unwrap();
        let has_mid_edge = |lines: &Value| {
            lines.as_array().unwrap().iter().any(|line| {
                line["points"].as_array().unwrap().windows(2).any(|pair| {
                    let x0 = pair[0][0].as_f64().unwrap();
                    let x1 = pair[1][0].as_f64().unwrap();
                    let y0 = pair[0][1].as_f64().unwrap();
                    let y1 = pair[1][1].as_f64().unwrap();
                    (x0 - 10.).abs() < 1e-6
                        && (x1 - 10.).abs() < 1e-6
                        && y0.min(y1) < -1.
                        && y0.max(y1) > 1.
                })
            })
        };
        assert!(!has_mid_edge(&occluded["visible"]));
        assert!(has_mid_edge(&occluded["hidden"]));

        let anchors = occluded["anchors"].as_array().unwrap();
        let first = anchors
            .iter()
            .find(|a| a["occurrence_id"] == original)
            .unwrap();
        let second = anchors
            .iter()
            .find(|a| {
                a["occurrence_id"] == added["id"]
                    && a["edge_id"] == first["edge_id"]
                    && a["endpoint"] == first["endpoint"]
            })
            .unwrap();
        let reference = |a: &Value| json!({"occurrence_id":a["occurrence_id"],"body_id":a["body_id"],"edge_id":a["edge_id"],"edge_key":a["edge_key"],"endpoint":a["endpoint"],"fallback_point":a["model_point"]});
        server
            .call_tool(
                "drawing_create_sheet",
                json!({"name":"Occurrence dimensions","format":"a4","orientation":"landscape"}),
            )
            .unwrap();
        let view = json!({"name":"Assembly top","kind":"top","scope":"assembly","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[90.,65.],"scale":2.});
        server
            .call_tool("drawing_add_view", json!({"sheet_id":1,"view":view}))
            .unwrap();
        let dimension = json!({"sheet_id":1,"view_id":1,"first":reference(first),"second":reference(second),"mode":"horizontal","offset":12.});
        server
            .call_tool("drawing_add_linear_dimension", dimension.clone())
            .unwrap();
        let exported = server
            .call_tool("drawing_export", json!({"sheet_id":1,"format":"svg"}))
            .unwrap();
        assert!(exported["content"]
            .as_str()
            .unwrap()
            .contains(">5.00 mm</text>"));
        let mut selected_view = view;
        selected_view["occurrence_ids"] = json!([original]);
        server
            .call_tool(
                "drawing_add_view",
                json!({"sheet_id":1,"view":selected_view}),
            )
            .unwrap();
        let saved = server.call_tool("drawing_document", json!({})).unwrap();
        let mut excluded = dimension.clone();
        excluded["view_id"] = json!(2);
        assert!(server
            .call_tool("drawing_add_linear_dimension", excluded)
            .is_err());
        let mut no_instance = dimension;
        no_instance["first"]["occurrence_id"] = Value::Null;
        assert!(server
            .call_tool("drawing_add_linear_dimension", no_instance)
            .is_err());
        assert_eq!(
            saved,
            server.call_tool("drawing_document", json!({})).unwrap()
        );
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        let before = server
            .call_tool("drawing_export", json!({"sheet_id":1,"format":"svg"}))
            .unwrap();
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool("cad_load_project_model", json!({"model_json":model}))
            .unwrap();
        assert_eq!(
            before["content"],
            restored
                .call_tool("drawing_export", json!({"sheet_id":1,"format":"svg"}))
                .unwrap()["content"]
        );
    }

    #[test]
    fn assembly_mesh_export_retains_repeated_occurrences_and_placement() {
        let (mut server, _) = mcp_box();
        let doc = server.call_tool("assembly_document", json!({})).unwrap();
        let component = doc["component_structure"]["occurrences"][0]["component_id"].clone();
        let added = server
            .call_tool(
                "assembly_create_occurrence",
                json!({"component_id":component,"name":"Second"}),
            )
            .unwrap();
        server.call_tool("assembly_set_occurrence_pose", json!({"occurrence_id":added["id"],"local_pose":{"translation":[100.,0.,0.],"rotation":[0.,0.,0.,1.]}})).unwrap();
        let before = server.call_tool("cad_project_model", json!({})).unwrap();
        let exported = server
            .call_tool("solid_export_3mf", json!({"slicer_target":"standard"}))
            .unwrap();
        let bytes = BASE64
            .decode(exported["bytes_base64"].as_str().unwrap())
            .unwrap();
        let meshes = limo_cad_export::test_reader::read_package(&bytes).unwrap();
        assert_eq!(meshes.len(), 2);
        let xml = pip_model_xml(&bytes);
        assert_eq!(xml.matches("<mesh>").count(), 1);
        limo_cad_export::validate_3mf_model_mesh(&parse_3mf_model_mesh(&xml)).unwrap();
        let max_x = meshes
            .iter()
            .flat_map(|m| m.vertices.iter().map(|p| p[0]))
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((max_x - 110.0).abs() < 1e-5);
        assert_eq!(
            server.call_tool("cad_project_model", json!({})).unwrap(),
            before
        );
    }

    #[test]
    fn exact_interference_distinguishes_overlap_touch_and_clearance() {
        let (mut server, _) = mcp_box();
        let document = server.call_tool("assembly_document", json!({})).unwrap();
        let component = document["component_structure"]["occurrences"][0]["component_id"].clone();
        let added = server
            .call_tool(
                "assembly_create_occurrence",
                json!({"component_id":component,"name":"Inspection fixture"}),
            )
            .unwrap();
        for (x, overlap, clearance) in [(10., 2000., 0.), (20., 0., 0.), (21., 0., 1.)] {
            server.call_tool("assembly_set_occurrence_pose", json!({"occurrence_id":added["id"],"local_pose":{"translation":[x,0.,0.],"rotation":[0.,0.,0.,1.]}})).unwrap();
            let before = server.call_tool("cad_project_model", json!({})).unwrap();
            let report = server
                .call_tool(
                    "assembly_interference_check",
                    json!({"clearance_threshold_mm":2.}),
                )
                .unwrap();
            assert_eq!(report["exact"], true);
            let pairs = report["pairs"].as_array().unwrap();
            assert_eq!(pairs.len(), 1);
            assert!((pairs[0]["overlap_volume_mm3"].as_f64().unwrap() - overlap).abs() < 1e-6);
            assert!((pairs[0]["minimum_clearance_mm"].as_f64().unwrap() - clearance).abs() < 1e-6);
            assert_eq!(pairs[0]["interfering"], overlap > 0.);
            assert_eq!(
                server.call_tool("cad_project_model", json!({})).unwrap(),
                before
            );
        }
        assert!(server
            .call_tool(
                "assembly_interference_check",
                json!({"clearance_threshold_mm":-1.})
            )
            .is_err());
        assert!(is_read_safe_while_attached("assembly_interference_check"));
        assert!(server
            .call_tool(
                "assembly_interference_check",
                json!({"occurrence_ids":[999999]})
            )
            .is_err());
    }

    #[test]
    fn project_visibility_uses_browser_state_without_changing_geometry() {
        assert!(is_read_safe_while_attached("project_visibility"));
        assert!(limo_cad_mcp_mutate::is_live_engine_query(
            "project_visibility"
        ));
        fn state(mut value: Value) -> Value {
            value.as_object_mut().unwrap().remove("_disclosure");
            value
        }
        let (mut server, update) = mcp_box();
        let body = update["scene"]["bodies"][0]["id"].clone();
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        let requested = json!({
            "hidden_body_ids":[body, body, 999999],
            "hidden_datum_plane_ids":[],
            "hidden_sketch_names":["Sketch1"]
        });
        let expected = parse_engine_envelope(host::handle(
            &mut server.manager,
            "project_set_visibility",
            &requested.to_string(),
        ))
        .unwrap();
        server
            .manager
            .set_project_visibility(Default::default())
            .unwrap();
        assert_eq!(
            interface::group_for("project_visibility"),
            Some("document/appearance")
        );
        let hidden = server
            .call_tool(
                "cad_interface",
                json!({
                    "action":"execute", "group":"document/appearance",
                    "operation":"project_set_visibility", "arguments":requested
                }),
            )
            .unwrap();
        let hidden = state(hidden);
        assert_eq!(hidden, expected);
        assert_eq!(hidden["hidden_body_ids"], json!([body]));
        let script_before_read = server.call_tool("cad_script", json!({})).unwrap();
        assert_eq!(
            state(server.call_tool("project_visibility", json!({})).unwrap()),
            hidden
        );
        assert_eq!(
            server.call_tool("cad_script", json!({})).unwrap(),
            script_before_read,
            "reading Browser visibility must not append a replay operation"
        );
        assert_eq!(server.call_tool("solid_scene", json!({})).unwrap(), scene);
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool("cad_load_project_model", json!({"model_json":model}))
            .unwrap();
        assert_eq!(
            state(restored.call_tool("project_visibility", json!({})).unwrap()),
            hidden
        );
        assert_eq!(restored.call_tool("solid_scene", json!({})).unwrap(), scene);
        let shown = restored
            .call_tool(
                "project_set_visibility",
                json!({
                    "hidden_body_ids":[], "hidden_datum_plane_ids":[], "hidden_sketch_names":[]
                }),
            )
            .unwrap();
        assert_eq!(shown["hidden_body_ids"], json!([]));
        assert_eq!(shown["hidden_sketch_names"], json!([]));
    }
    #[test]
    fn construction_visibility_matches_host_and_preserves_native_model() {
        fn visibility(value: Value) -> Value {
            serde_json::to_value(
                serde_json::from_value::<limo_cad_sketch::ProjectVisibilityDto>(value).unwrap(),
            )
            .unwrap()
        }
        let (mut server, update) = mcp_box();
        let body_id = update["scene"]["bodies"][0]["id"].as_u64().unwrap();
        let planes = server
            .call_tool(
                "construction_plane_offset",
                json!({
                    "name":"Stock reference", "reference":{"type":"origin_plane","plane":"xy"},
                    "distance":5.
                }),
            )
            .unwrap();
        let datum_id = planes["planes"][0]["datum_id"].as_u64().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"name":"Layout", "plane":{
                    "type":"datum_plane", "datum_id":datum_id
                }}),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        let initial = limo_cad_sketch::ProjectVisibilityDto {
            hidden_body_ids: vec![body_id],
            ..Default::default()
        };
        server
            .manager
            .set_project_visibility(initial.clone())
            .unwrap();
        let before = server.call_tool("cad_project_model", json!({})).unwrap();
        let mut before: Value = serde_json::from_str(before.as_str().unwrap()).unwrap();
        let scene = server.call_tool("solid_scene", json!({})).unwrap();

        let host_result = parse_engine_envelope(host::handle(
            &mut server.manager,
            "construction_set_visibility",
            r#"{"visible":false}"#,
        ))
        .unwrap();
        server.manager.set_project_visibility(initial).unwrap();
        assert_eq!(
            interface::group_for("construction_set_visibility"),
            Some("solid/reference")
        );
        let grouped = |visible: bool| {
            json!({"action":"execute", "group":"solid/reference",
            "operation":"construction_set_visibility", "arguments":{"visible":visible}})
        };
        let hidden = visibility(server.call_tool("cad_interface", grouped(false)).unwrap());
        assert_eq!(hidden, host_result);
        assert_eq!(hidden["hidden_body_ids"], json!([body_id]));
        assert_eq!(hidden["hidden_sketch_names"], json!(["Layout", "Sketch1"]));
        assert_eq!(hidden["hidden_datum_plane_ids"], json!([datum_id]));
        let after = server.call_tool("cad_project_model", json!({})).unwrap();
        let mut after: Value = serde_json::from_str(after.as_str().unwrap()).unwrap();
        before.as_object_mut().unwrap().remove("visibility");
        after.as_object_mut().unwrap().remove("visibility");
        assert_eq!(
            after, before,
            "visibility must not edit parametric intent or IDs"
        );
        assert_eq!(server.call_tool("solid_scene", json!({})).unwrap(), scene);

        let selected = visibility(
            server
                .call_tool(
                    "construction_set_visibility",
                    json!({
                        "visible":true, "sketch_names":["Sketch1", "Sketch1"]
                    }),
                )
                .unwrap(),
        );
        assert_eq!(selected["hidden_sketch_names"], json!(["Layout"]));
        assert_eq!(selected["hidden_datum_plane_ids"], json!([datum_id]));
        assert_eq!(
            visibility(
                server
                    .call_tool(
                        "construction_set_visibility",
                        json!({
                        "visible":true, "sketch_names":[]
                                })
                    )
                    .unwrap()
            ),
            selected,
            "an explicit empty selection is a no-op"
        );
        for arguments in [
            json!({"visible":true, "sketch_names":["Layout", "Missing"]}),
            json!({"visible":true, "sketch_names":["Layout"], "datum_plane_ids":[999999]}),
        ] {
            assert!(server
                .call_tool("construction_set_visibility", arguments)
                .is_err());
            assert_eq!(
                serde_json::to_value(server.manager.project_visibility()).unwrap(),
                selected
            );
        }
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool("cad_load_project_model", json!({"model_json":model}))
            .unwrap();
        assert_eq!(
            serde_json::to_value(restored.manager.project_visibility()).unwrap(),
            selected
        );
        assert_eq!(restored.call_tool("solid_scene", json!({})).unwrap(), scene);

        restored
            .call_tool(
                "sketch_begin",
                json!({"name":"In progress", "plane":{
                    "type":"origin_plane", "plane":"xz"
                }}),
            )
            .unwrap();
        let active = serde_json::to_value(restored.manager.active_snapshot()).unwrap();
        let hidden = restored.call_tool("cad_interface", grouped(false)).unwrap();
        assert_eq!(
            serde_json::to_value(restored.manager.active_snapshot()).unwrap(),
            active
        );
        assert!(!hidden["hidden_sketch_names"]
            .as_array()
            .unwrap()
            .contains(&json!("In progress")));
        restored.call_tool("sketch_finish", json!({})).unwrap();
        assert!(
            !restored
                .manager
                .project_visibility()
                .hidden_sketch_names
                .contains(&"In progress".into()),
            "new references start visible even after an earlier hide-all"
        );
        let shown = restored.call_tool("cad_interface", grouped(true)).unwrap();
        assert_eq!(shown["hidden_body_ids"], json!([body_id]));
        assert_eq!(shown["hidden_sketch_names"], json!([]));
        assert_eq!(shown["hidden_datum_plane_ids"], json!([]));
    }

    #[test]
    fn named_sketches_and_datums_preserve_references_and_reject_duplicates() {
        let mut server = CadServer::new().unwrap();
        let plane = server.call_tool("construction_plane_offset",json!({"name":"Stock / A face","reference":{"type":"origin_plane","plane":"xy"},"distance":0.})).unwrap();
        assert_eq!(plane["planes"][0]["name"], "Stock / A face");
        let reference = json!({"type":"datum_plane","datum_id":plane["planes"][0]["datum_id"]});
        let sketch = server
            .call_tool("sketch_begin", json!({"name":"Sketch2","plane":reference}))
            .unwrap();
        assert_eq!(sketch["name"], "Sketch2");
        server.call_tool("sketch_finish", json!({})).unwrap();
        let before = server.call_tool("cad_project_model", json!({})).unwrap();
        for name in ["Sketch2", "  ", "bad\nname"] {
            assert!(server
                .call_tool("sketch_begin", json!({"name":name,"plane":reference}))
                .is_err());
            assert_eq!(
                server.call_tool("cad_project_model", json!({})).unwrap(),
                before
            );
        }
        assert!(server.call_tool("construction_plane_offset",json!({"name":"Stock / A face","reference":{"type":"origin_plane","plane":"xy"},"distance":5.})).is_err());
        assert_eq!(
            server.call_tool("cad_project_model", json!({})).unwrap(),
            before
        );
        let automatic = server
            .call_tool("sketch_begin", json!({"plane":reference}))
            .unwrap();
        assert_eq!(automatic["name"], "Sketch3");
        server.call_tool("sketch_finish", json!({})).unwrap();
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool("cad_load_project_model", json!({"model_json":model}))
            .unwrap();
        let edit = restored
            .call_tool("sketch_edit", json!({"name":"Sketch2"}))
            .unwrap();
        assert_eq!(edit["plane"], reference);
    }

    #[test]
    fn occt_box_export_3mf_preserves_indexed_closed_mesh() {
        let (mut server, _) = mcp_box();
        let request = MeshExportRequest::default();
        let raw_meshes = server
            .kernel
            .tessellate_bodies(&request)
            .expect("OCCT tessellation should succeed for a simple box");
        assert_eq!(raw_meshes.len(), 1);
        let raw = &raw_meshes[0];
        let raw_vertex_count = raw.positions.len() / 3;
        let tri_count = raw.triangle_count();
        assert_eq!(tri_count, 12, "a box should have two triangles per face");
        assert_eq!(
            raw_vertex_count, 8,
            "native topology should share box corners"
        );
        limo_cad_export::validate_3mf_model_mesh(raw)
            .expect("native box mesh should already be closed, outward-facing and nondegenerate");

        let exported = server
            .call_tool("solid_export_3mf", json!({"slicer_target": "standard"}))
            .expect("3MF export should succeed");
        let bytes = BASE64
            .decode(exported["bytes_base64"].as_str().unwrap())
            .unwrap();
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("3MF should be a zip");
        let mut model = archive.by_name("3D/3dmodel.model").unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut model, &mut xml).unwrap();

        let vertex_count = xml.matches("<vertex ").count();
        let triangle_count = xml.matches("<triangle ").count();
        assert_eq!(triangle_count, tri_count);
        assert_eq!(
            vertex_count, raw_vertex_count,
            "3MF export should preserve the eight shared native corners"
        );

        let parsed = parse_3mf_model_mesh(&xml);
        assert_eq!(parsed.positions.len() / 3, vertex_count);
        assert_eq!(parsed.triangle_count(), triangle_count);
        assert_eq!(
            limo_cad_export::boundary_edge_count(&parsed),
            0,
            "exported 3MF mesh should be manifold (no boundary edges)"
        );
        assert_eq!(
            limo_cad_export::invalid_model_edge_count(&parsed),
            0,
            "every exported edge should have two oppositely oriented triangle uses"
        );
        limo_cad_export::validate_3mf_model_mesh(&parsed)
            .expect("exported box mesh should retain positive volume and nondegenerate triangles");
        let corners = |mesh: &limo_cad_export::TriangleMesh| {
            let mut points = mesh.positions.as_chunks::<3>().0.to_vec();
            points.sort_by(|a, b| {
                a[0].total_cmp(&b[0])
                    .then(a[1].total_cmp(&b[1]))
                    .then(a[2].total_cmp(&b[2]))
            });
            points
        };
        let expected_corners: Vec<_> = [-10., 10.]
            .into_iter()
            .flat_map(|x| {
                [-10., 10.]
                    .into_iter()
                    .flat_map(move |y| [0., 10.].into_iter().map(move |z| [x, y, z]))
            })
            .collect();
        assert_eq!(corners(raw), expected_corners, "native box dimensions");
        assert_eq!(corners(&parsed), corners(raw), "3MF preserves box geometry");
    }

    #[test]
    fn set_body_appearance_from_preset_then_exports_3mf() {
        let (mut server, update) = mcp_box();
        let body_id = update["scene"]["bodies"][0]["id"]
            .as_u64()
            .expect("extrude returns a body id");
        let assigned = server
            .call_tool(
                "set_body_appearance",
                json!({
                    "body_id": body_id,
                    "preset_id": "bambu.pla.basic.red"
                }),
            )
            .expect("preset appearance assign");
        let appearances = assigned["body_appearances"].as_array().unwrap();
        assert_eq!(appearances.len(), 1);
        assert_eq!(appearances[0]["preset_id"], "bambu.pla.basic.red");
        assert_eq!(appearances[0]["brand"], "Bambu Lab");
        let listed = server.call_tool("body_appearances", json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1);
        let exported = server
            .call_tool("solid_export_3mf", json!({"slicer_target": "bambu_studio"}))
            .unwrap();
        assert_eq!(
            &BASE64
                .decode(exported["bytes_base64"].as_str().unwrap())
                .unwrap()[0..2],
            b"PK"
        );
    }

    #[test]
    fn material_presets_match_through_headless_and_queued_native_dispatch() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-material-parity-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_one_box_session(&unique);
        let body = scene["bodies"][0]["id"].clone();
        let original = session::require_model_json(&unique).unwrap();
        let mut owning_native_host = CadServer::new().unwrap();
        owning_native_host
            .call_tool("cad_load_project_model", json!({"model_json":original}))
            .unwrap();
        let mut headless = CadServer::new().unwrap();
        headless
            .call_tool("cad_load_project_model", json!({"model_json":original}))
            .unwrap();
        let mut attached = CadServer::new().unwrap();
        attached
            .call_tool("cad_attach", json!({"session_id":unique}))
            .unwrap();
        for (preset, channel, color_name) in [
            ("bambu.petg.hf.black", 24, "Black"),
            ("bambu.petg.hf.white", 245, "White"),
        ] {
            let arguments = json!({"body_id":body,"preset_id":preset});
            headless
                .call_tool("set_body_appearance", arguments.clone())
                .unwrap();
            let generation = session::read_heartbeat_generation(&unique).unwrap();
            attached.call_tool("cad_submit",json!({"name":"set_body_appearance","arguments":arguments,"base_generation":generation})).unwrap();
            session::apply_inbox_op(&unique, |name, arguments| {
                let spec = limo_cad_mcp_mutate::lookup_mutate(name).unwrap();
                let encoded = limo_cad_mcp_mutate::encode_payload(spec.payload, &arguments)?;
                let result = parse_engine_envelope(host::handle(
                    &mut owning_native_host.manager,
                    spec.engine_method,
                    &encoded,
                ))?;
                session::publish_applied_snapshot(
                    &unique,
                    &owning_native_host
                        .manager
                        .export_project_model()
                        .map_err(|error| error.to_string())?,
                )?;
                Ok(result)
            })
            .unwrap();
            let actual = owning_native_host.manager.export_project_model().unwrap();
            assert_eq!(
                actual,
                headless.manager.export_project_model().unwrap(),
                "full model parity for {preset}"
            );
            assert_eq!(
                session::require_model_json(&unique).unwrap(),
                actual,
                "published live snapshot must retain the resolved material"
            );
            let model: Value = serde_json::from_str(&actual).unwrap();
            let appearance = &model["body_appearances"][0];
            assert_eq!(
                appearance["color"],
                json!({"r":channel,"g":channel,"b":channel,"a":255})
            );
            assert_eq!(appearance["material_name"], "Bambu PETG HF");
            assert_eq!(appearance["filament_type"], "PETG");
            assert_eq!(appearance["brand"], "Bambu Lab");
            assert_eq!(appearance["color_name"], color_name);
            assert_eq!(appearance["filament_id"], "GFG00");
            assert_eq!(appearance["density_g_cm3"], 1.27);
            let mut reopened = CadServer::new().unwrap();
            reopened
                .call_tool("cad_load_project_model", json!({"model_json":actual}))
                .unwrap();
            assert_eq!(
                reopened.manager.export_project_model().unwrap(),
                headless.manager.export_project_model().unwrap()
            );
        }
        let extract = |export: Value| {
            let bytes = BASE64
                .decode(export["bytes_base64"].as_str().unwrap())
                .unwrap();
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut model = String::new();
            std::io::Read::read_to_string(
                &mut archive.by_name("3D/3dmodel.model").unwrap(),
                &mut model,
            )
            .unwrap();
            model
        };
        let live_mesh = extract(
            owning_native_host
                .call_tool("solid_export_3mf", json!({"slicer_target":"bambu_studio"}))
                .unwrap(),
        );
        let headless_mesh = extract(
            headless
                .call_tool("solid_export_3mf", json!({"slicer_target":"bambu_studio"}))
                .unwrap(),
        );
        assert_eq!(live_mesh, headless_mesh);
        assert!(live_mesh.contains("F5F5F5"));

        let before = owning_native_host.manager.export_project_model().unwrap();
        let generation = session::read_heartbeat_generation(&unique).unwrap();
        let invalid = json!({"body_id":body,"preset_id":"missing-material"});
        assert!(headless
            .call_tool("set_body_appearance", invalid.clone())
            .is_err());
        attached.call_tool("cad_submit",json!({"name":"set_body_appearance","arguments":invalid,"base_generation":generation})).unwrap();
        let error = session::apply_inbox_op(&unique, |name, arguments| {
            let spec = limo_cad_mcp_mutate::lookup_mutate(name).unwrap();
            let encoded = limo_cad_mcp_mutate::encode_payload(spec.payload, &arguments)?;
            parse_engine_envelope(host::handle(
                &mut owning_native_host.manager,
                spec.engine_method,
                &encoded,
            ))
        })
        .unwrap_err();
        assert!(error.contains("unknown material preset_id"));
        assert_eq!(
            owning_native_host.manager.export_project_model().unwrap(),
            before
        );
        assert_eq!(headless.manager.export_project_model().unwrap(), before);
        assert_eq!(
            session::read_heartbeat_generation(&unique).unwrap(),
            generation
        );
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn solid_export_step_returns_base64_payload() {
        let (mut server, _) = mcp_box();
        let exported = server
            .call_tool("solid_export_step", json!({}))
            .expect("STEP export should succeed for a simple box");
        assert_eq!(exported["format"], "step");
        assert_eq!(exported["encoding"], "base64");
        assert!(exported["bytes_base64"].as_str().unwrap().len() > 16);
    }

    #[test]
    fn mcp_import_step_records_forward_script() {
        let (mut donor, _) = mcp_box();
        let exported = donor
            .call_tool("solid_export_step", json!({}))
            .expect("STEP export should succeed for a simple box");
        let data_base64 = exported["bytes_base64"]
            .as_str()
            .expect("STEP export returns bytes_base64")
            .to_string();

        let mut server = CadServer::new().unwrap();
        let imported = server
            .call_tool(
                "solid_import_step",
                json!({
                    "file_name": "box.step",
                    "data_base64": data_base64,
                }),
            )
            .expect("solid_import_step should import the exported box");
        assert!(
            imported["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            imported["scene"]["errors"]
        );
        assert_eq!(imported["scene"]["bodies"].as_array().unwrap().len(), 1);

        let compared = server
            .call_tool("cad_compare_solids", json!({}))
            .expect("cad_compare_solids summarizes the imported scene");
        assert_eq!(compared["body_count"], 1);
        assert!(compared["bodies"][0]["triangle_count"].as_u64().unwrap() > 0);

        let script = server
            .call_tool("cad_script", json!({}))
            .expect("cad_script dumps the forward tool trace");
        let calls = script["calls"].as_array().expect("cad_script.calls");
        assert!(
            calls.iter().any(|call| call["name"] == "solid_import_step"),
            "cad_script should contain solid_import_step, got {calls:?}"
        );
        assert!(
            calls
                .iter()
                .all(|call| call["name"] != "cad_script" && call["name"] != "cad_compare_solids"),
            "cad_script must skip itself and read-only compare"
        );
    }

    #[test]
    fn cad_script_after_attach_refresh_replays_on_fresh_server() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-script-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (mut donor, _) = mcp_box();
        let model = donor.call_tool("cad_project_model", json!({})).unwrap();
        let model_json = model
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&model).unwrap());
        session::write_session(&unique, "model.json", &model_json).unwrap();
        session::write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .expect("attach snapshot for script regression");
        server
            .call_tool("cad_refresh", json!({}))
            .expect("refresh snapshot for script regression");

        let scene = server
            .call_tool("solid_scene", json!({}))
            .expect("attached snapshot has a solid scene");
        let body_id = scene["bodies"][0]["id"].clone();

        server
            .call_tool("cad_detach", json!({}))
            .expect("detach before headless mutate for script regression");
        let mirrored = server
            .call_tool(
                "solid_mirror",
                json!({
                    "body_ids": [body_id],
                    "plane": {"type": "origin_plane", "plane": "yz"}
                }),
            )
            .expect("modeling mutate after attach/refresh/detach");
        assert_eq!(mirrored["scene"]["bodies"].as_array().unwrap().len(), 2);

        let script = server
            .call_tool("cad_script", json!({}))
            .expect("cad_script dumps portable modeling ops");
        let calls = script["calls"].as_array().expect("cad_script.calls");
        assert_eq!(
            calls.first().and_then(|call| call["name"].as_str()),
            Some("cad_load_project_model"),
            "attach/refresh must seed cad_load_project_model baseline, got {calls:?}"
        );
        assert!(
            calls.iter().any(|call| call["name"] == "solid_mirror"),
            "cad_script should contain solid_mirror, got {calls:?}"
        );
        assert!(
            calls.iter().all(|call| {
                !matches!(
                    call["name"].as_str(),
                    Some("cad_attach" | "cad_refresh" | "cad_detach" | "solid_scene")
                )
            }),
            "cad_script must omit session-control and inspect helpers, got {calls:?}"
        );
        let script_text = serde_json::to_string(&script).unwrap();
        assert!(
            !script_text.contains(&unique),
            "portable cad_script must not embed the ephemeral session UUID, got {script_text}"
        );

        let expected = server
            .call_tool("cad_compare_solids", json!({}))
            .expect("compare solids on attached+modeled server");

        let mut fresh = CadServer::new().unwrap();
        for call in calls {
            let name = call["name"].as_str().expect("script call name");
            let arguments = call.get("arguments").cloned().unwrap_or(Value::Null);
            fresh
                .call_tool(name, arguments)
                .unwrap_or_else(|error| panic!("fresh replay of {name} failed: {error}"));
        }
        let replayed = fresh
            .call_tool("cad_compare_solids", json!({}))
            .expect("compare solids after fresh script replay");
        assert_eq!(
            replayed["body_count"], expected["body_count"],
            "replayed body_count should match attached session"
        );
        assert_eq!(
            replayed["bodies"], expected["bodies"],
            "replayed solid metrics should match attached session"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_solid_move_copy_translates_body() {
        let (mut server, base) = mcp_box();
        let body_id = base["scene"]["bodies"][0]["id"].clone();
        let moved = server
            .call_tool(
                "solid_move_copy",
                json!({
                    "body_ids": [body_id],
                    "translation": {"x": 12.0, "y": -4.0, "z": 2.0},
                    "rotation": [0.0, 0.0, 0.0, 1.0],
                    "pivot": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "copy": false
                }),
            )
            .expect("solid_move_copy should translate the box");
        assert!(
            moved["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            moved["scene"]["errors"]
        );
        assert_eq!(moved["scene"]["bodies"].as_array().unwrap().len(), 1);

        let copied = server
            .call_tool(
                "solid_move_copy",
                json!({
                    "body_ids": [moved["scene"]["bodies"][0]["id"].clone()],
                    "translation": {"x": 25.0, "y": 0.0, "z": 0.0},
                    "pivot": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "copy": true
                }),
            )
            .expect("solid_move_copy copy=true should leave source and create a body");
        assert!(
            copied["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            copied["scene"]["errors"]
        );
        assert_eq!(copied["scene"]["bodies"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn mcp_sketch_patterns_are_one_step_engine_operations() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        let source = server
            .call_tool(
                "sketch_add_line",
                json!({
                    "from": {"x": 10.0, "y": 0.0},
                    "to_raw": {"x": 20.0, "y": 0.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        let source_id = source["entity_id"].clone();

        let rectangular = server
            .call_tool(
                "sketch_rectangular_pattern",
                json!({
                    "entity_ids": [source_id],
                    "direction": {"x": 0.0, "y": 1.0},
                    "spacing": 10.0,
                    "count": 3
                }),
            )
            .unwrap();
        assert_eq!(
            rectangular["sketch"]["entities"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entity| entity["kind"] == "line")
                .count(),
            3
        );

        let circular = server
            .call_tool(
                "sketch_circular_pattern",
                json!({
                    "entity_ids": [source["entity_id"].clone()],
                    "center": {"x": 0.0, "y": 0.0},
                    "count": 4,
                    "total_angle_deg": 360.0
                }),
            )
            .unwrap();
        assert_eq!(
            circular["sketch"]["entities"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entity| entity["kind"] == "line")
                .count(),
            6
        );
    }

    #[test]
    fn mcp_tools_build_and_revolve_a_real_occt_body() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": 10.0, "y": 0.0},
                    "p2": {"x": 20.0, "y": 15.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();

        let update = server
            .call_tool(
                "solid_revolve",
                json!({
                    "sketch_name": "Sketch1",
                    "profile_indices": [0],
                    "axis_origin": {"x": 0.0, "y": 0.0},
                    "axis_direction": {"x": 0.0, "y": 1.0},
                    "axis_line_entity_id": null,
                    "angle_deg": 360.0,
                    "flip": false,
                    "operation": "new_body",
                    "target_body_ids": []
                }),
            )
            .unwrap();
        assert_eq!(update["scene"]["bodies"].as_array().unwrap().len(), 1);
        assert_eq!(update["document"]["features"][1]["kind"], "revolve");
        assert!(update["scene"]["bodies"][0]["mesh"]["indices"]
            .as_array()
            .is_some_and(|indices| !indices.is_empty()));

        let project = server.call_tool("cad_project_model", json!({})).unwrap();
        let model: Value = serde_json::from_str(project.as_str().unwrap()).unwrap();
        assert_eq!(model["revolves"].as_array().unwrap().len(), 1);

        let summary = server.feature_summary(&json!({})).unwrap();
        let native_body = &server.manager.solid_scene_ref().bodies[0];
        let native_topology = json!({
            "faces": native_body.faces.iter().map(|face| json!({
                "id": face.id.0,
                "outer_shell": face.outer_shell,
                "plane": face.plane,
                "cylinder": face.cylinder,
                "edge_keys": face.edge_keys,
                "linear_seam_edge_keys": face.linear_seam_edge_keys,
            })).collect::<Vec<_>>(),
            "edges": native_body.edges.iter().map(|edge| json!({
                "key": edge.key,
                "circle": edge.circle,
                "refinable": edge.refinable,
                "first": edge.points.first(),
                "last": edge.points.last(),
            })).collect::<Vec<_>>(),
        });
        assert!(
            native_body
                .faces
                .iter()
                .all(|face| face.outer_shell == Some(true)),
            "Revolved tube lacks native exterior-shell evidence: {native_topology}"
        );
        assert_eq!(
            summary["hole_count"], 1,
            "revolved cavity must be counted as one hole"
        );
        assert_eq!(summary["holes"][0]["diameter"], 20.0);
        assert_eq!(summary["holes"][0]["style"], "simple");
        assert_eq!(
            summary["holes"][0]["through"], true,
            "Revolved tube extent was not recognized: {summary}; topology: {native_topology}"
        );
        assert_eq!(summary["holes"][0]["depth"], 15.0);
        assert_eq!(
            summary["hole_detection_scope"],
            "closed_cylindrical_cavities"
        );
        let inferred = summary::summarize(server.manager.solid_scene_ref(), &[]);
        let evidence = inferred.json(true);
        assert_eq!(evidence["hole_candidate_count"], 1);
        assert_eq!(evidence["authored_hole_count"], 0);
        assert_eq!(evidence["holes"][0]["confidence"], "candidate");
        assert_eq!(evidence["holes"][0]["source"], "geometry");
        assert_eq!(
            evidence["holes"][0]["through_evidence"],
            "native_outer_shell_and_two_analytic_annular_openings"
        );
        assert!(evidence["holes"][0]["face_ids"]
            .as_array()
            .is_some_and(|ids| !ids.is_empty()));
        assert_eq!(
            evidence["holes"][0]["inference_method"],
            "closed_analytic_circle_and_inward_display_normals"
        );
        let hole = &inferred.holes[0];
        for through in [false, true] {
            let checked = summary::check(
                &inferred,
                &json!({"holes": [{"x": hole.position[0], "y": hole.position[1], "through": through}]}),
                0.1,
            )
            .unwrap();
            assert_eq!(checked["ok"], through, "{checked}");
        }
        // The same local geometry from a legacy producer is not exterior
        // passage evidence. Both expectations must reject unknown topology.
        let mut legacy = server.manager.solid_scene_ref().clone();
        for face in &mut legacy.bodies[0].faces {
            face.outer_shell = None;
        }
        let unknown = summary::summarize(&legacy, &[]);
        assert_eq!(unknown.holes[0].through, None);
        let unknown_hole = &unknown.holes[0];
        for through in [false, true] {
            let checked = summary::check(
                &unknown,
                &json!({"holes": [{"x": unknown_hole.position[0], "y": unknown_hole.position[1], "through": through}]}),
                0.1,
            )
            .unwrap();
            assert_eq!(checked["ok"], false, "{checked}");
        }
        let mut split_wall = server.manager.solid_scene_ref().clone();
        let body = &mut split_wall.bodies[0];
        let inner = body
            .faces
            .iter()
            .find(|face| {
                face.cylinder
                    .is_some_and(|cylinder| cylinder.radius == 10.0)
            })
            .unwrap()
            .clone();
        body.faces.push(inner);
        let inferred = summary::holes_from_scene(&split_wall);
        assert_eq!(inferred.len(), 1);
        assert_eq!(inferred[0].style, "simple");
        assert!(inferred[0].counterbore_diameter.is_none());
        let mut incomplete = server.manager.solid_scene_ref().clone();
        for body in &mut incomplete.bodies {
            body.mesh.normals.clear();
        }
        assert!(summary::holes_from_scene(&incomplete).is_empty());
        let mut partial = server.manager.solid_scene_ref().clone();
        for body in &mut partial.bodies {
            for edge in &mut body.edges {
                if let Some(circle) = &mut edge.circle {
                    circle.closed = false;
                }
            }
        }
        assert!(summary::holes_from_scene(&partial).is_empty());

        let mut overlapping = summary::summarize(server.manager.solid_scene_ref(), &[]);
        let mut candidate = overlapping.holes[0].clone();
        candidate.position[0] += 1.0;
        overlapping.holes.push(candidate);
        let warnings = summary::warnings(&overlapping, &[], &[]);
        let overlap = warnings
            .iter()
            .find(|warning| warning["code"] == "holes_overlap")
            .unwrap();
        assert_eq!(overlap["confirmed"], false);
        assert_eq!(overlap["evidence"], "projected_axis_distance");
        assert!(overlap["message"]
            .as_str()
            .unwrap()
            .contains("verify axial extents"));

        let mut restored = CadServer::new().unwrap();
        let restored_update = restored
            .call_tool(
                "cad_load_project_model",
                json!({"model_json": project.as_str().unwrap()}),
            )
            .unwrap();
        assert_eq!(
            restored_update["scene"]["bodies"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            restored_update["document"]["features"][1]["kind"],
            "revolve"
        );
    }

    #[test]
    fn mcp_tools_create_solid_fillets_chamfers_and_holes() {
        for (tool, value_name) in [("solid_fillet", "radius"), ("solid_chamfer", "distance")] {
            let (mut server, base) = mcp_box();
            let body = &base["scene"]["bodies"][0];
            let edge_ids = vec![
                body["edges"][0]["id"].clone(),
                body["edges"][1]["id"].clone(),
            ];
            let mut request = Map::new();
            request.insert("body_id".to_string(), body["id"].clone());
            request.insert("edge_ids".to_string(), Value::Array(edge_ids));
            request.insert(value_name.to_string(), json!(1.0));
            request.insert("tangent_chain".to_string(), json!(false));
            let update = server.call_tool(tool, Value::Object(request)).unwrap();
            assert!(update["scene"]["errors"].as_array().unwrap().is_empty());
            assert_eq!(update["scene"]["bodies"].as_array().unwrap().len(), 1);
            let definitions = server
                .call_tool(
                    if tool == "solid_fillet" {
                        "solid_fillet_definitions"
                    } else {
                        "solid_chamfer_definitions"
                    },
                    json!({}),
                )
                .unwrap();
            assert_eq!(definitions.as_array().unwrap().len(), 1);
            let summary = server.feature_summary(&json!({})).unwrap();
            assert_eq!(
                summary["hole_count"], 0,
                "{tool} must not create inferred holes"
            );
        }

        let (mut server, base) = mcp_box();
        let body = &base["scene"]["bodies"][0];
        let top = body["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|face| {
                face["plane"]["normal"][2]
                    .as_f64()
                    .is_some_and(|normal_z| normal_z > 0.9)
            })
            .unwrap();
        let origin = top["plane"]["origin"].as_array().unwrap();
        let u = top["plane"]["u"].as_array().unwrap();
        let v = top["plane"]["v"].as_array().unwrap();
        let delta = [
            -origin[0].as_f64().unwrap(),
            -origin[1].as_f64().unwrap(),
            10.0 - origin[2].as_f64().unwrap(),
        ];
        let project = |axis: &Vec<Value>| {
            delta
                .iter()
                .zip(axis)
                .map(|(component, basis)| component * basis.as_f64().unwrap())
                .sum::<f64>()
        };
        let update = server
            .call_tool(
                "solid_hole",
                json!({
                    "body_id": body["id"].clone(),
                    "face_id": top["id"].clone(),
                    "position": {"x": project(u), "y": project(v)},
                    "diameter": 5.0,
                    "extent": {"type": "through_all"},
                    "style": "countersink",
                    "counterbore_diameter": 0.0,
                    "counterbore_depth": 0.0,
                    "countersink_diameter": 8.0,
                    "countersink_angle_deg": 90.0,
                    "thread": {
                        "standard": "iso_metric",
                        "series": "metric_coarse",
                        "designation": "M6 x 1 - 6H",
                        "class": "6H",
                        "nominal_diameter": 6.0,
                        "pitch": 1.0,
                        "threads_per_inch": null,
                        "hand": "right",
                        "depth": null,
                        "representation": "modeled",
                        "tap_drill_designation": "5 mm"
                    },
                    "flip": false
                }),
            )
            .unwrap();
        assert!(
            update["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            update["scene"]["errors"]
        );
        assert_eq!(update["document"]["features"][2]["kind"], "hole");
        let definitions = server
            .call_tool("solid_hole_definitions", json!({}))
            .unwrap();
        assert_eq!(definitions.as_array().unwrap().len(), 1);
        assert_eq!(definitions[0]["thread"]["designation"], "M6 x 1 - 6H");
        assert_eq!(definitions[0]["thread"]["representation"], "modeled");
        let replay = server.call_tool("solid_recompute", json!({})).unwrap();
        assert!(replay["scene"]["errors"].as_array().unwrap().is_empty());
    }

    #[test]
    fn mcp_construction_planes_and_body_operations_run_through_native_occt() {
        let (mut split_server, split_base) = mcp_box();
        let plane = split_server
            .call_tool(
                "construction_plane_offset",
                json!({
                    "reference": {"type": "origin_plane", "plane": "xy"},
                    "distance": 5.0
                }),
            )
            .unwrap();
        let datum_id = plane["planes"][0]["datum_id"].clone();
        assert_eq!(plane["planes"][0]["basis"]["origin"][2], json!(5.0));
        let split = split_server
            .call_tool(
                "solid_split_body",
                json!({
                    "body_id": split_base["scene"]["bodies"][0]["id"].clone(),
                    "plane": {"type": "datum_plane", "datum_id": datum_id}
                }),
            )
            .unwrap();
        assert!(split["scene"]["errors"].as_array().unwrap().is_empty());
        assert_eq!(split["scene"]["bodies"].as_array().unwrap().len(), 2);

        let (mut shell_server, shell_base) = mcp_box();
        let shell_body = &shell_base["scene"]["bodies"][0];
        let shell_face = shell_body["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|face| {
                face["plane"]["normal"][2]
                    .as_f64()
                    .is_some_and(|normal| normal > 0.9)
            })
            .unwrap()["id"]
            .clone();
        let shell = shell_server
            .call_tool(
                "solid_shell",
                json!({
                    "body_id": shell_body["id"].clone(),
                    "face_ids": [shell_face],
                    "thickness": 1.0,
                    "inward": true
                }),
            )
            .unwrap();
        assert!(shell["scene"]["errors"].as_array().unwrap().is_empty());
        assert_eq!(shell["scene"]["bodies"].as_array().unwrap().len(), 1);

        let (mut mirror_server, mirror_base) = mcp_box();
        let mirror = mirror_server
            .call_tool(
                "solid_mirror",
                json!({
                    "body_ids": [mirror_base["scene"]["bodies"][0]["id"].clone()],
                    "plane": {"type": "origin_plane", "plane": "yz"}
                }),
            )
            .unwrap();
        assert_eq!(mirror["scene"]["bodies"].as_array().unwrap().len(), 2);

        let (mut rectangular_server, rectangular_base) = mcp_box();
        let rectangular = rectangular_server
            .call_tool(
                "solid_rectangular_pattern",
                json!({
                    "body_ids": [rectangular_base["scene"]["bodies"][0]["id"].clone()],
                    "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
                    "spacing": 30.0,
                    "count": 3,
                    "second_direction": null,
                    "second_spacing": 0.0,
                    "second_count": 1
                }),
            )
            .unwrap();
        assert_eq!(rectangular["scene"]["bodies"].as_array().unwrap().len(), 3);

        let (mut circular_server, circular_base) = mcp_box();
        let circular = circular_server
            .call_tool(
                "solid_circular_pattern",
                json!({
                    "body_ids": [circular_base["scene"]["bodies"][0]["id"].clone()],
                    "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "axis_direction": {"x": 0.0, "y": 0.0, "z": 1.0},
                    "count": 4,
                    "total_angle_deg": 360.0
                }),
            )
            .unwrap();
        assert_eq!(circular["scene"]["bodies"].as_array().unwrap().len(), 4);

        let mirror_bodies = mirror["scene"]["bodies"].as_array().unwrap();
        let combined = mirror_server
            .call_tool(
                "solid_combine",
                json!({
                    "target_body_id": mirror_bodies[0]["id"].clone(),
                    "tool_body_ids": [mirror_bodies[1]["id"].clone()],
                    "operation": "join",
                    "keep_tools": false
                }),
            )
            .unwrap();
        assert!(combined["scene"]["errors"].as_array().unwrap().is_empty());
        assert_eq!(combined["scene"]["bodies"].as_array().unwrap().len(), 1);
    }

    fn mcp_patterned_box() -> (CadServer, Value) {
        let (mut server, base) = mcp_box();
        let patterned = server
            .call_tool(
                "solid_rectangular_pattern",
                json!({
                    "body_ids":[base["scene"]["bodies"][0]["id"]],
                    "direction":{"x":1.,"y":0.,"z":0.},"spacing":10.,"count":3,
                    "second_direction":null,"second_spacing":0.,"second_count":1
                }),
            )
            .unwrap();
        (server, patterned)
    }

    #[test]
    fn consumed_pattern_components_clean_up_and_rollback_preserves_intent() {
        let (mut server, patterned) = mcp_patterned_box();
        let bodies = patterned["scene"]["bodies"].as_array().unwrap();
        let pattern_end = patterned["document"]["features"].as_array().unwrap().len();
        let before = server.call_tool("assembly_document", json!({})).unwrap();
        let combined = server.call_tool("solid_combine", json!({"target_body_id":bodies[0]["id"],"tool_body_ids":[bodies[1]["id"],bodies[2]["id"]],"operation":"join","keep_tools":false})).unwrap();
        assert_eq!(combined["scene"]["errors"], json!([]));
        assert_eq!(combined["scene"]["bodies"].as_array().unwrap().len(), 1);
        let after = server.call_tool("assembly_document", json!({})).unwrap();
        assert_eq!(
            after["component_structure"]["definitions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            after["component_structure"]["occurrences"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let saved = server.manager.export_project_model().unwrap();

        let mut legacy: Value = serde_json::from_str(&saved).unwrap();
        legacy["assembly"]["component_structure"] = before["component_structure"].clone();
        let mut loaded = CadServer::new().unwrap();
        loaded
            .call_tool(
                "cad_load_project_model",
                json!({"model_json":legacy.to_string()}),
            )
            .unwrap();
        assert_eq!(
            loaded.call_tool("assembly_document", json!({})).unwrap(),
            after
        );
        assert_eq!(
            loaded.call_tool("solid_scene", json!({})).unwrap()["bodies"],
            combined["scene"]["bodies"]
        );

        server
            .call_tool("solid_set_rollback", json!({"rollback_index":pattern_end}))
            .unwrap();
        assert_eq!(
            server.call_tool("solid_scene", json!({})).unwrap()["bodies"],
            patterned["scene"]["bodies"]
        );
        let restored_pattern = server.call_tool("assembly_document", json!({})).unwrap();
        assert_eq!(
            restored_pattern["component_structure"]["definitions"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        server
            .call_tool(
                "solid_set_rollback",
                json!({"rollback_index":pattern_end-1}),
            )
            .unwrap();
        assert_eq!(
            server.call_tool("solid_scene", json!({})).unwrap()["bodies"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            server.call_tool("assembly_document", json!({})).unwrap(),
            restored_pattern,
            "rollback absence must preserve default identities for redo"
        );
        let rollback_model = server.manager.export_project_model().unwrap();
        let mut reopened_rollback = CadServer::new().unwrap();
        reopened_rollback
            .call_tool(
                "cad_load_project_model",
                json!({"model_json":rollback_model}),
            )
            .unwrap();
        assert_eq!(
            reopened_rollback
                .call_tool("assembly_document", json!({}))
                .unwrap(),
            restored_pattern,
            "save/reopen at an earlier marker must retain later component identities"
        );
        server
            .call_tool("solid_set_rollback", json!({"rollback_index":pattern_end}))
            .unwrap();
        assert_eq!(
            server.call_tool("assembly_document", json!({})).unwrap(),
            restored_pattern
        );
        server
            .call_tool(
                "solid_set_rollback",
                json!({"rollback_index":pattern_end+1}),
            )
            .unwrap();
        assert_eq!(
            server.call_tool("assembly_document", json!({})).unwrap()["component_structure"]
                ["definitions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn consumed_components_referenced_by_drawing_selections_and_edges_are_preserved() {
        for reference_kind in ["body_selection", "occurrence_selection", "annotation"] {
            let (mut server, patterned) = mcp_patterned_box();
            let bodies = patterned["scene"]["bodies"].as_array().unwrap();
            let referenced_body = &bodies[1]["id"];
            let assembly = server.call_tool("assembly_document", json!({})).unwrap();
            let definition = assembly["component_structure"]["definitions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|definition| definition["body_ids"] == json!([referenced_body]))
                .unwrap();
            let occurrence = assembly["component_structure"]["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .find(|occurrence| occurrence["component_id"] == definition["id"])
                .unwrap();
            server
                .call_tool(
                    "drawing_create_sheet",
                    json!({"name":"Keep tooling intent","format":"a4","orientation":"landscape"}),
                )
                .unwrap();
            let mut view = json!({"name":"Tooling","kind":"top","scope":"assembly","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[90.,65.],"scale":1.});
            if reference_kind == "body_selection" {
                view["body_ids"] = json!([referenced_body]);
            }
            if reference_kind == "occurrence_selection" {
                view["occurrence_ids"] = json!([occurrence["id"]]);
            }
            server
                .call_tool("drawing_add_view", json!({"sheet_id":1,"view":view}))
                .unwrap();
            if reference_kind == "annotation" {
                let projection = server.call_tool("drawing_projection", json!({"scope":"assembly","body_ids":[referenced_body],"direction":[0.,0.,1.],"up":[0.,1.,0.]})).unwrap();
                let anchors = projection["anchors"].as_array().unwrap();
                let first = &anchors[0];
                let second = anchors
                    .iter()
                    .find(|anchor| anchor["point"][0] != first["point"][0])
                    .unwrap();
                let reference = |anchor: &Value| json!({"body_id":anchor["body_id"],"occurrence_id":anchor["occurrence_id"],"edge_id":anchor["edge_id"],"edge_key":anchor["edge_key"],"endpoint":anchor["endpoint"],"fallback_point":anchor["model_point"]});
                server.call_tool("drawing_add_linear_dimension", json!({"sheet_id":1,"view_id":1,"first":reference(first),"second":reference(second),"mode":"horizontal","offset":12.})).unwrap();
            }
            server.call_tool("solid_combine", json!({"target_body_id":bodies[0]["id"],"tool_body_ids":[bodies[1]["id"],bodies[2]["id"]],"operation":"join","keep_tools":false})).unwrap();
            let after = server.call_tool("assembly_document", json!({})).unwrap();
            assert_eq!(
                after["component_structure"]["definitions"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert!(
                after["component_structure"]["definitions"]
                    .as_array()
                    .unwrap()
                    .contains(definition),
                "preserve {reference_kind}"
            );
            assert!(
                after["component_structure"]["occurrences"]
                    .as_array()
                    .unwrap()
                    .contains(occurrence),
                "preserve {reference_kind}"
            );
        }
    }

    #[test]
    fn mcp_curved_and_guided_sweeps_run_through_native_occt() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": -10.0, "y": -10.0},
                    "p2": {"x": 10.0, "y": 10.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();

        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "yz"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_arc_center",
                json!({
                    "center": {"x": 0.0, "y": 20.0},
                    "start": {"x": 0.0, "y": 0.0},
                    "sweep": {"x": 20.0, "y": 20.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_arc_center",
                json!({
                    "center": {"x": -20.0, "y": 0.0},
                    "start": {"x": 0.0, "y": 0.0},
                    "sweep": {"x": -20.0, "y": 20.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();

        let catalog = server.call_tool("sketch_profiles", json!({})).unwrap();
        let arcs = catalog
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["sketch_name"] == "Sketch2")
            .unwrap()["path_curves"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|curve| curve["kind"] == "arc")
            .map(|curve| curve["entity_id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(arcs.len(), 2);

        let invalid = server
            .call_tool(
                "solid_sweep",
                json!({
                    "profile": {"sketch_name":"Sketch1", "profile_index":0},
                    "path_sketch_name":"Sketch2", "path_entity_ids":[arcs[0].clone()],
                    "operation":"new_body", "target_body_ids":[], "guide_rail":null,
                    "orientation":"corrected_frenet", "transition":"round_corner", "force_c1":true
                }),
            )
            .unwrap();
        let failure = &invalid["scene"]["errors"][0];
        assert!(failure["message"]
            .as_str()
            .unwrap()
            .contains("Place the profile across the path"));
        assert!(invalid["scene"]["bodies"].as_array().unwrap().is_empty());
        server
            .call_tool(
                "solid_delete_feature",
                json!({"feature_id":failure["feature_id"]}),
            )
            .unwrap();

        let update = server
            .call_tool(
                "solid_sweep",
                json!({
                    "profile": {"sketch_name": "Sketch1", "profile_index": 0},
                    "path_sketch_name": "Sketch2",
                    "path_entity_ids": [arcs[1].clone()],
                    "operation": "new_body",
                    "target_body_ids": [],
                    "guide_rail": null,
                    "orientation": "corrected_frenet",
                    "transition": "round_corner",
                    "force_c1": true
                }),
            )
            .unwrap();
        assert!(
            update["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            update["scene"]["errors"]
        );
        assert_eq!(update["scene"]["bodies"].as_array().unwrap().len(), 1);
        assert!(update["scene"]["bodies"][0]["mesh"]["indices"]
            .as_array()
            .is_some_and(|indices| !indices.is_empty()));
        let definitions = server
            .call_tool("solid_sweep_definitions", json!({}))
            .unwrap();
        assert_eq!(definitions[0]["orientation"], "corrected_frenet");
        assert_eq!(definitions[0]["transition"], "round_corner");
        assert_eq!(definitions[0]["force_c1"], true);
        assert!(definitions[0]["guide_rail"].is_null());
        let print = server
            .call_tool("solid_export_3mf", json!({"slicer_target":"standard"}))
            .expect("curved sweep boundaries must form a closed printable mesh");
        assert!(print["bytes_base64"].as_str().unwrap().len() > 32);

        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "yz"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_line",
                json!({
                    "from": {"x": 0.0, "y": 0.0},
                    "to_raw": {"x": 0.0, "y": 30.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_line",
                json!({
                    "from": {"x": 10.0, "y": 0.0},
                    "to_raw": {"x": 10.0, "y": 30.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        let catalog = server.call_tool("sketch_profiles", json!({})).unwrap();
        let lines = catalog
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["sketch_name"] == "Sketch3")
            .unwrap()["path_curves"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|curve| curve["kind"] == "line")
            .map(|curve| curve["entity_id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);

        let guided = server
            .call_tool(
                "solid_sweep",
                json!({
                    "profile": {"sketch_name": "Sketch1", "profile_index": 0},
                    "path_sketch_name": "Sketch3",
                    "path_entity_ids": [lines[0].clone()],
                    "operation": "new_body",
                    "target_body_ids": [],
                    "guide_rail": {
                        "sketch_name": "Sketch3",
                        "entity_ids": [lines[1].clone()]
                    },
                    "orientation": "corrected_frenet",
                    "transition": "transformed",
                    "force_c1": true
                }),
            )
            .unwrap();
        assert!(
            guided["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            guided["scene"]["errors"]
        );
        assert_eq!(guided["scene"]["bodies"].as_array().unwrap().len(), 2);
        let definitions = server
            .call_tool("solid_sweep_definitions", json!({}))
            .unwrap();
        assert_eq!(definitions.as_array().unwrap().len(), 2);
        assert!(definitions[1]["guide_rail"].is_object());
        let print = server
            .call_tool("solid_export_3mf", json!({"slicer_target":"standard"}))
            .expect("guided and retained curved sweeps must both remain closed");
        assert!(print["bytes_base64"].as_str().unwrap().len() > 32);
    }

    #[test]
    fn mcp_guided_g2_loft_runs_through_native_occt() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": -10.0, "y": -10.0},
                    "p2": {"x": 10.0, "y": 10.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        let plane = server
            .call_tool(
                "construction_plane_offset",
                json!({
                    "reference": {"type": "origin_plane", "plane": "xy"},
                    "distance": 30.0
                }),
            )
            .unwrap();
        let datum_id = plane["planes"][0]["datum_id"].clone();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "datum_plane", "datum_id": datum_id}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": -10.0, "y": -10.0},
                    "p2": {"x": 10.0, "y": 10.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();

        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xz"}}),
            )
            .unwrap();
        for x in [0.0, 10.0] {
            server
                .call_tool(
                    "sketch_add_line",
                    json!({
                        "from": {"x": x, "y": 0.0},
                        "to_raw": {"x": x, "y": 30.0},
                        "ctrl_held": false
                    }),
                )
                .unwrap();
        }
        server.call_tool("sketch_finish", json!({})).unwrap();
        let catalog = server.call_tool("sketch_profiles", json!({})).unwrap();
        let lines = catalog
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["sketch_name"] == "Sketch3")
            .unwrap()["path_curves"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|curve| curve["kind"] == "line")
            .map(|curve| curve["entity_id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);

        let update = server
            .call_tool(
                "solid_loft",
                json!({
                    "sections": [
                        {"sketch_name": "Sketch1", "profile_index": 0},
                        {"sketch_name": "Sketch2", "profile_index": 0}
                    ],
                    "ruled": false,
                    "operation": "new_body",
                    "target_body_ids": [],
                    "continuity": "g2",
                    "centerline": {
                        "sketch_name": "Sketch3",
                        "entity_ids": [lines[0].clone()]
                    },
                    "guide_rail": {
                        "sketch_name": "Sketch3",
                        "entity_ids": [lines[1].clone()]
                    }
                }),
            )
            .unwrap();
        assert!(
            update["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            update["scene"]["errors"]
        );
        assert_eq!(update["scene"]["bodies"].as_array().unwrap().len(), 1);
        let definitions = server
            .call_tool("solid_loft_definitions", json!({}))
            .unwrap();
        assert_eq!(definitions[0]["continuity"], "g2");
        assert!(definitions[0]["centerline"].is_object());
        assert!(definitions[0]["guide_rail"].is_object());
    }

    #[test]
    fn mcp_curved_rib_and_reference_extents_run_through_native_occt() {
        let mut curved_server = CadServer::new().unwrap();
        curved_server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        curved_server
            .call_tool(
                "sketch_add_arc_center",
                json!({
                    "center": {"x": 0.0, "y": 0.0},
                    "start": {"x": -20.0, "y": 0.0},
                    "sweep": {"x": 0.0, "y": 20.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        curved_server.call_tool("sketch_finish", json!({})).unwrap();
        let catalog = curved_server
            .call_tool("sketch_profiles", json!({}))
            .unwrap();
        let arc_id = catalog[0]["path_curves"]
            .as_array()
            .unwrap()
            .iter()
            .find(|curve| curve["kind"] == "arc")
            .unwrap()["entity_id"]
            .clone();
        let curved = curved_server
            .call_tool(
                "solid_rib",
                json!({
                    "sketch_name": "Sketch1",
                    "line_entity_ids": [arc_id],
                    "thickness": 2.0,
                    "depth": 5.0,
                    "extent": {"type": "distance", "depth": 5.0},
                    "symmetric": false,
                    "flip": false,
                    "operation": "new_body",
                    "target_body_ids": []
                }),
            )
            .unwrap();
        assert!(
            curved["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            curved["scene"]["errors"]
        );
        assert_eq!(curved["scene"]["bodies"].as_array().unwrap().len(), 1);

        let add_target_rib_sketch = |server: &mut CadServer| {
            server
                .call_tool(
                    "sketch_begin",
                    json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
                )
                .unwrap();
            server
                .call_tool(
                    "sketch_add_line",
                    json!({
                        "from": {"x": -10.0, "y": 0.0},
                        "to_raw": {"x": 10.0, "y": 0.0},
                        "ctrl_held": false
                    }),
                )
                .unwrap();
            server.call_tool("sketch_finish", json!({})).unwrap();
            let catalog = server.call_tool("sketch_profiles", json!({})).unwrap();
            catalog
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["sketch_name"] == "Sketch2")
                .unwrap()["path_curves"][0]["entity_id"]
                .clone()
        };

        let (mut next_server, next_base) = mcp_box();
        let next_body_id = next_base["scene"]["bodies"][0]["id"].clone();
        let next_line_id = add_target_rib_sketch(&mut next_server);
        let to_next = next_server
            .call_tool(
                "solid_rib",
                json!({
                    "sketch_name": "Sketch2",
                    "line_entity_ids": [next_line_id],
                    "thickness": 2.0,
                    "depth": 5.0,
                    "extent": {"type": "to_next"},
                    "symmetric": false,
                    "flip": false,
                    "operation": "join",
                    "target_body_ids": [next_body_id]
                }),
            )
            .unwrap();
        assert!(
            to_next["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            to_next["scene"]["errors"]
        );
        assert_eq!(to_next["scene"]["bodies"].as_array().unwrap().len(), 1);

        let (mut face_server, face_base) = mcp_box();
        let face_body = &face_base["scene"]["bodies"][0];
        let face_body_id = face_body["id"].clone();
        let top_face_id = face_body["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|face| {
                face["plane"]["normal"][2]
                    .as_f64()
                    .is_some_and(|normal| normal > 0.9)
            })
            .unwrap()["id"]
            .clone();
        let face_line_id = add_target_rib_sketch(&mut face_server);
        let to_face = face_server
            .call_tool(
                "solid_rib",
                json!({
                    "sketch_name": "Sketch2",
                    "line_entity_ids": [face_line_id],
                    "thickness": 2.0,
                    "depth": 5.0,
                    "extent": {"type": "to_face", "face_id": top_face_id},
                    "symmetric": false,
                    "flip": false,
                    "operation": "join",
                    "target_body_ids": [face_body_id]
                }),
            )
            .unwrap();
        assert!(
            to_face["scene"]["errors"].as_array().unwrap().is_empty(),
            "{}",
            to_face["scene"]["errors"]
        );
        assert_eq!(to_face["scene"]["bodies"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn assembly_component_occurrence_grounded_roundtrip() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": -5.0, "y": -5.0},
                    "p2": {"x": 5.0, "y": 5.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        let extruded = server
            .call_tool(
                "solid_extrude",
                json!({
                    "sketch_name": "Sketch1",
                    "profile_indices": [0],
                    "operation": "new_body",
                    "extent": {"type": "distance", "distance": 8.0},
                    "taper_angle_deg": 0.0,
                    "flip": false,
                    "target_body_ids": []
                }),
            )
            .unwrap();
        let body_id = extruded["scene"]["bodies"][0]["id"]
            .as_u64()
            .expect("extruded body id");

        let component = server
            .call_tool(
                "assembly_create_component",
                json!({
                    "name": "Block",
                    "body_ids": [body_id],
                    "absorb_promoted_bodies": true
                }),
            )
            .expect("create component");
        assert_eq!(component["name"], "Block");
        let component_id = component["id"].as_u64().expect("component id");
        assert!(
            component["body_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id.as_u64() == Some(body_id)),
            "component should own extruded body: {component}"
        );

        let document = server
            .call_tool("assembly_document", json!({}))
            .expect("assembly document");
        let definitions = document["component_structure"]["definitions"]
            .as_array()
            .expect("definitions");
        assert!(
            definitions
                .iter()
                .any(|definition| definition["id"].as_u64() == Some(component_id)
                    && definition["name"] == "Block"),
            "assembly_document missing component: {document}"
        );
        let occurrence = document["component_structure"]["occurrences"]
            .as_array()
            .expect("occurrences")
            .iter()
            .find(|occurrence| occurrence["component_id"].as_u64() == Some(component_id))
            .cloned()
            .expect("occurrence for Block");
        let occurrence_id = occurrence["id"].as_u64().expect("occurrence id");

        server
            .call_tool(
                "assembly_set_occurrence_pose",
                json!({
                    "occurrence_id": occurrence_id,
                    "local_pose": {
                        "translation": [10.0, 0.0, 0.0],
                        "rotation": [0.0, 0.0, 0.0, 1.0]
                    }
                }),
            )
            .expect("set pose");
        let grounded = server
            .call_tool(
                "assembly_set_occurrence_grounded",
                json!({
                    "occurrence_id": occurrence_id,
                    "grounded": true
                }),
            )
            .expect("set grounded");
        let grounded_occurrence = grounded["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["id"].as_u64() == Some(occurrence_id))
            .unwrap();
        assert_eq!(grounded_occurrence["grounded"], true);
        assert_eq!(
            grounded_occurrence["local_pose"]["translation"][0]
                .as_f64()
                .unwrap(),
            10.0
        );

        let inspect = server
            .call_tool("assembly_document", json!({}))
            .expect("re-inspect");
        assert!(
            inspect["component_structure"]["definitions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|definition| definition["name"] == "Block"),
            "component missing after ground: {inspect}"
        );
        let solution = server
            .call_tool("assembly_solution", json!({}))
            .expect("assembly solution");
        assert!(
            solution.get("occurrence_poses").is_some()
                || solution.get("body_poses").is_some()
                || solution.as_object().map(|o| !o.is_empty()).unwrap_or(false),
            "expected a non-empty assembly solution: {solution}"
        );
    }

    fn parse_lock_error(error: &str) -> Value {
        serde_json::from_str(error).unwrap_or_else(|_| json!({ "raw": error }))
    }

    fn assert_session_read_only(error: &str) {
        let parsed = parse_lock_error(error);
        assert_eq!(parsed["code"], "session_read_only");
        assert_eq!(parsed["writeback"], false);
        assert_eq!(parsed["session_mode"], "read_only_snapshot");
    }

    #[test]
    fn attach_direct_mutate_rejected_submit_accepted_detach_restores() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-lock-submit-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update, _) = write_box_session(&unique);
        let body_id = update["scene"]["bodies"][0]["id"].clone();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();

        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        assert!(!scene["bodies"].as_array().unwrap().is_empty());

        let mutate_err = server
            .call_tool("solid_mirror", solid_mirror_args(&body_id))
            .expect_err("direct mutate must fail while attached");
        assert_session_read_only(&mutate_err);
        assert!(server.attached_document_id.is_some());

        let appearance_err = server
            .call_tool(
                "set_body_appearance",
                json!({"body_id": body_id, "preset_id": "generic.pla"}),
            )
            .expect_err("appearance write must fail while attached");
        assert_session_read_only(&appearance_err);

        let writeback_err = server
            .call_tool(
                "cad_attach",
                json!({"session_id": unique, "writeback": true}),
            )
            .expect_err("writeback:true attach must fail");
        let wb = parse_lock_error(&writeback_err);
        assert_eq!(wb["code"], "writeback_rejected");
        assert_eq!(wb["writeback"], false);

        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_mirror",
                    "arguments": solid_mirror_args(&body_id),
                    "base_generation": 1
                }),
            )
            .expect("cad_submit must be accepted while attached");
        assert_eq!(submitted["submitted"], true);
        assert_eq!(submitted["seq"], 1);
        assert_eq!(submitted["applied"], false);

        server.call_tool("cad_detach", json!({})).unwrap();
        assert!(server.attached_document_id.is_none());
        server
            .call_tool("solid_mirror", solid_mirror_args(&body_id))
            .expect("direct mutate must succeed after cad_detach");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_create_component_accepted_by_cad_submit_classifier() {
        assert!(
            limo_cad_mcp_mutate::lookup_mutate("assembly_create_component").is_some(),
            "assembly_create_component must be in shared mutate map"
        );
        assert!(is_modeling_mutate("assembly_create_component"));
        assert!(!is_read_safe_while_attached("assembly_create_component"));
        assert!(is_read_safe_while_attached("assembly_document"));
        assert!(is_read_safe_while_attached("assembly_solution"));
        assert!(!is_modeling_mutate("assembly_document"));
        assert!(!is_modeling_mutate("assembly_solution"));
    }

    #[test]
    fn tool_spec_mutates_match_shared_inbox_map() {
        let mut missing = Vec::new();
        let mut mismatched = Vec::new();
        for spec in tool_specs() {
            if spec.execution == Execution::Control || is_read_safe_while_attached(spec.name) {
                assert!(
                    limo_cad_mcp_mutate::lookup_mutate(spec.name).is_none(),
                    "read-safe/control {} must not be an inbox mutate",
                    spec.name
                );
                continue;
            }
            let Some(shared) = limo_cad_mcp_mutate::lookup_mutate(spec.name) else {
                missing.push(spec.name);
                continue;
            };
            if shared.engine_method != spec.engine_method {
                mismatched.push(format!(
                    "{} engine_method {} != {}",
                    spec.name, shared.engine_method, spec.engine_method
                ));
            }
            let expected_exec = match spec.execution {
                Execution::Direct => limo_cad_mcp_mutate::ExecutionKind::Direct,
                Execution::SolidReplay => limo_cad_mcp_mutate::ExecutionKind::SolidReplay,
                Execution::Control => unreachable!(),
            };
            if shared.execution != expected_exec {
                mismatched.push(format!("{} execution mismatch", spec.name));
            }
        }
        assert!(
            missing.is_empty(),
            "ToolSpec mutates missing from shared map: {missing:?}"
        );
        assert!(
            mismatched.is_empty(),
            "ToolSpec/shared map mismatches: {mismatched:?}"
        );
        assert_eq!(
            limo_cad_mcp_mutate::mutate_specs().len(),
            tool_specs()
                .iter()
                .filter(|spec| spec.execution != Execution::Control
                    && !is_read_safe_while_attached(spec.name))
                .count()
        );
    }

    #[test]
    fn assembly_update_component_rename_preserves_bodies_and_lcs() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": -5.0, "y": -5.0},
                    "p2": {"x": 5.0, "y": 5.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        let extruded = server
            .call_tool(
                "solid_extrude",
                json!({
                    "sketch_name": "Sketch1",
                    "profile_indices": [0],
                    "operation": "new_body",
                    "extent": {"type": "distance", "distance": 6.0},
                    "taper_angle_deg": 0.0,
                    "flip": false,
                    "target_body_ids": []
                }),
            )
            .unwrap();
        let body_id = extruded["scene"]["bodies"][0]["id"]
            .as_u64()
            .expect("extruded body id");

        let component = server
            .call_tool(
                "assembly_create_component",
                json!({
                    "name": "Stock",
                    "body_ids": [body_id],
                    "local_coordinate_system": {
                        "translation": [1.0, 2.0, 3.0],
                        "rotation": [0.0, 0.0, 0.0, 1.0]
                    },
                    "absorb_promoted_bodies": true
                }),
            )
            .expect("create component");
        let component_id = component["id"].as_u64().expect("component id");
        let body_ids_before = component["body_ids"].clone();
        let lcs_before = component["local_coordinate_system"].clone();
        let promoted_before = component["promoted"].clone();

        let renamed = server
            .call_tool(
                "assembly_update_component",
                json!({
                    "component": {
                        "id": component_id,
                        "name": "StockRenamed"
                    }
                }),
            )
            .expect("rename-only component update");
        assert_eq!(renamed["name"], "StockRenamed");
        assert_eq!(renamed["body_ids"], body_ids_before);
        assert_eq!(renamed["local_coordinate_system"], lcs_before);
        assert_eq!(renamed["promoted"], promoted_before);

        let document = server
            .call_tool("assembly_document", json!({}))
            .expect("assembly document");
        let definition = document["component_structure"]["definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["id"].as_u64() == Some(component_id))
            .expect("renamed definition");
        assert_eq!(definition["name"], "StockRenamed");
        assert_eq!(definition["body_ids"], body_ids_before);
        assert_eq!(definition["local_coordinate_system"], lcs_before);
        assert_eq!(definition["promoted"], promoted_before);
    }

    #[test]
    fn assembly_update_occurrence_rename_preserves_pose_parent_flags() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": -5.0, "y": -5.0},
                    "p2": {"x": 5.0, "y": 5.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        let extruded = server
            .call_tool(
                "solid_extrude",
                json!({
                    "sketch_name": "Sketch1",
                    "profile_indices": [0],
                    "operation": "new_body",
                    "extent": {"type": "distance", "distance": 4.0},
                    "taper_angle_deg": 0.0,
                    "flip": false,
                    "target_body_ids": []
                }),
            )
            .unwrap();
        let body_id = extruded["scene"]["bodies"][0]["id"]
            .as_u64()
            .expect("extruded body id");
        let component = server
            .call_tool(
                "assembly_create_component",
                json!({
                    "name": "Part",
                    "body_ids": [body_id],
                    "absorb_promoted_bodies": true
                }),
            )
            .expect("create component");
        let component_id = component["id"].as_u64().expect("component id");

        let document = server
            .call_tool("assembly_document", json!({}))
            .expect("assembly document");
        let occurrence_id = document["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|occurrence| occurrence["component_id"].as_u64() == Some(component_id))
            .and_then(|occurrence| occurrence["id"].as_u64())
            .expect("occurrence id");

        server
            .call_tool(
                "assembly_set_occurrence_pose",
                json!({
                    "occurrence_id": occurrence_id,
                    "local_pose": {
                        "translation": [7.0, 8.0, 9.0],
                        "rotation": [0.0, 0.0, 0.0, 1.0]
                    }
                }),
            )
            .expect("set pose");
        server
            .call_tool(
                "assembly_set_occurrence_grounded",
                json!({
                    "occurrence_id": occurrence_id,
                    "grounded": true
                }),
            )
            .expect("set grounded");

        let before = server
            .call_tool("assembly_document", json!({}))
            .expect("document before rename");
        let occurrence_before = before["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|occurrence| occurrence["id"].as_u64() == Some(occurrence_id))
            .cloned()
            .expect("occurrence before rename");
        let parent_before = occurrence_before["parent_occurrence_id"].clone();
        let pose_before = occurrence_before["local_pose"].clone();
        let visible_before = occurrence_before["visible"].clone();
        let grounded_before = occurrence_before["grounded"].clone();
        let component_before = occurrence_before["component_id"].clone();

        let renamed = server
            .call_tool(
                "assembly_update_occurrence",
                json!({
                    "occurrence": {
                        "id": occurrence_id,
                        "name": "PartRenamed"
                    }
                }),
            )
            .expect("rename-only occurrence update");
        assert_eq!(renamed["name"], "PartRenamed");
        assert_eq!(renamed["component_id"], component_before);
        assert_eq!(renamed["parent_occurrence_id"], parent_before);
        assert_eq!(renamed["local_pose"], pose_before);
        assert_eq!(renamed["visible"], visible_before);
        assert_eq!(renamed["grounded"], grounded_before);

        let after = server
            .call_tool("assembly_document", json!({}))
            .expect("document after rename");
        let occurrence_after = after["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|occurrence| occurrence["id"].as_u64() == Some(occurrence_id))
            .expect("occurrence after rename");
        assert_eq!(occurrence_after["name"], "PartRenamed");
        assert_eq!(occurrence_after["component_id"], component_before);
        assert_eq!(occurrence_after["parent_occurrence_id"], parent_before);
        assert_eq!(occurrence_after["local_pose"], pose_before);
        assert_eq!(occurrence_after["visible"], visible_before);
        assert_eq!(occurrence_after["grounded"], grounded_before);
    }

    #[test]
    fn occurrence_rename_after_joint_create_keeps_occurrence_ids() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let _second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"] != body_a["id"])
            .cloned()
            .expect("body B");
        let created = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeOccRename",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(&body_a),
                    "connector_b": planar_connector_from_body(&body_b),
                    "grounded_body_id": body_a["id"]
                }),
            )
            .unwrap();
        let joint_id = created["id"].as_u64().expect("joint id");
        let occ_a = created["advanced"]["connector_a_occurrence_id"]
            .as_u64()
            .expect("occ A");
        let occ_b = created["advanced"]["connector_b_occurrence_id"]
            .as_u64()
            .expect("occ B");
        assert_ne!(occ_a, occ_b);

        server
            .call_tool(
                "assembly_update_occurrence",
                json!({"occurrence": {"id": occ_a, "name": "RenamedA"}}),
            )
            .expect("rename occ A");
        server
            .call_tool(
                "assembly_update_occurrence",
                json!({"occurrence": {"id": occ_b, "name": "RenamedB"}}),
            )
            .expect("rename occ B");

        let document = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&document, joint_id, "HingeOccRename");
        let joint = document["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .expect("joint after rename");
        assert_eq!(joint["advanced"]["connector_a_occurrence_id"], occ_a);
        assert_eq!(joint["advanced"]["connector_b_occurrence_id"], occ_b);
        let names: Vec<&str> = document["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|occurrence| occurrence["name"].as_str())
            .collect();
        assert!(
            names.contains(&"RenamedA") && names.contains(&"RenamedB"),
            "occurrence display names must change: {names:?}"
        );
        assert_ne!(
            joint["name"], "RenamedA",
            "joint must keep its own name, not the occurrence display name"
        );
    }

    #[test]
    fn attached_sketch_reads_use_the_live_engine_without_loading_a_stale_model() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-live-read-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_box_session(&unique);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":unique}))
            .unwrap();
        assert!(server.manager.active_snapshot().is_none());
        let peer = unique.clone();
        let host = std::thread::spawn(move || {
            let mut manager = SketchManager::new();
            parse_engine_envelope(host::handle(
                &mut manager,
                "begin_sketch",
                r#"{"plane":{"type":"origin_plane","plane":"xy"}}"#,
            ))
            .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Ok(entries) =
                    std::fs::read_dir(session::session_dir().join(&peer).join("controls"))
                {
                    for entry in entries.flatten() {
                        if !entry
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".request.json")
                        {
                            continue;
                        }
                        let request: Value =
                            serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap())
                                .unwrap();
                        let query = &request["sketch_query"];
                        let method = query["method"].as_str().unwrap();
                        assert!(limo_cad_mcp_mutate::is_live_engine_query(method));
                        let value = parse_engine_envelope(host::handle(
                            &mut manager,
                            method,
                            query["payload"].as_str().unwrap(),
                        ))
                        .unwrap();
                        session::write_session(
                            &peer,
                            &format!("controls/{}.result.json", request["id"].as_str().unwrap()),
                            &json!({"status":"applied","value":value}).to_string(),
                        )
                        .unwrap();
                        return value;
                    }
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        let result = server
            .call_tool("sketch_eval_expression", json!({"text":"1200 / 5"}))
            .unwrap();
        assert_eq!(result["value"], host.join().unwrap()["value"]);
        assert_eq!(result["value"], 240.0);
        assert!(server.manager.active_snapshot().is_none());
        for mutate in limo_cad_mcp_mutate::mutate_specs() {
            assert!(!limo_cad_mcp_mutate::is_live_engine_query(
                mutate.engine_method
            ));
        }
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn grouped_interface_uses_engine_results_and_rejects_wrong_groups() {
        let mut direct = CadServer::new().unwrap();
        let mut grouped = CadServer::new().unwrap();
        let arguments = json!({"plane":{"type":"origin_plane","plane":"xy"}});
        let expected = direct.call_tool("sketch_begin", arguments.clone()).unwrap();
        assert!(grouped
            .call_tool(
                "cad_interface",
                json!({"action":"execute","group":"solid/build",
            "operation":"sketch_begin","arguments":arguments})
            )
            .is_err());
        assert!(grouped.manager.active_snapshot().is_none());
        let actual = grouped
            .call_tool(
                "cad_interface",
                json!({"action":"execute","group":"sketch/draw",
            "operation":"sketch_begin","arguments":arguments}),
            )
            .unwrap();
        assert_eq!(actual, expected);

        grouped
            .call_tool("cad_interface", json!({"action":"catalog"}))
            .unwrap();
        let script = grouped.call_tool("cad_script", json!({})).unwrap();
        assert_eq!(script["calls"].as_array().unwrap().len(), 1);
        let mut replay = CadServer::new().unwrap();
        for call in script["calls"].as_array().unwrap() {
            replay
                .call_tool(call["name"].as_str().unwrap(), call["arguments"].clone())
                .unwrap();
        }
        assert!(replay.manager.active_snapshot().is_some());
    }

    #[test]
    fn acknowledged_file_open_replaces_same_session_read_model() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-open-replacement-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_box_session(&unique);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":unique}))
            .unwrap();
        assert_eq!(server.manager.solid_scene().bodies.len(), 1);
        let mut replacement = CadServer::new().unwrap();
        replacement
            .call_tool(
                "cad_set_document_name",
                json!({"name":"Opened replacement"}),
            )
            .unwrap();
        let replacement_json = replacement
            .call_tool("cad_project_model", json!({}))
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        let peer = unique.clone();
        let host = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Ok(entries) =
                    std::fs::read_dir(session::session_dir().join(&peer).join("controls"))
                {
                    for entry in entries.flatten() {
                        if !entry
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".request.json")
                        {
                            continue;
                        }
                        let request: Value =
                            serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap())
                                .unwrap();
                        assert_eq!(request["ui"]["command"], "open");
                        session::write_session(&peer, "model.json", &replacement_json).unwrap();
                        session::write_session(
                            &peer,
                            "heartbeat.json",
                            &json!({"updated_ms":session::now_ms(),"generation":2}).to_string(),
                        )
                        .unwrap();
                        session::write_session(
                            &peer,
                            &format!("controls/{}.result.json", request["id"].as_str().unwrap()),
                            &json!({"status":"applied","active_session_id":peer}).to_string(),
                        )
                        .unwrap();
                        return;
                    }
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        let result = server
            .call_tool(
                "cad_interface",
                json!({"action":"file","command":"open","path":"replacement.limo"}),
            )
            .unwrap();
        host.join().unwrap();
        assert_eq!(result["attached_session_id"], unique);
        let status = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(status["attached_generation"], 2);
        assert_eq!(status["stale"], false);
        assert_eq!(
            server.call_tool("cad_document", json!({})).unwrap()["name"],
            "Opened replacement"
        );
        assert!(
            server.call_tool("solid_scene", json!({})).unwrap()["bodies"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            server.call_tool("cad_project_model", json!({})).unwrap(),
            replacement
                .call_tool("cad_project_model", json!({}))
                .unwrap()
        );
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn slow_file_open_acknowledges_and_attaches_replacement_session() {
        let _guard = session::env_lock();
        let original = session::test_session_uuid();
        let replacement = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-slow-open-{original}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_box_session(&original);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":original}))
            .unwrap();
        let mut opened = CadServer::new().unwrap();
        opened
            .call_tool(
                "cad_set_document_name",
                json!({"name":"Slow opened replacement"}),
            )
            .unwrap();
        let model = opened.call_tool("cad_project_model", json!({})).unwrap();
        let source = original.clone();
        let target = replacement.clone();
        let expected_model = model.clone();
        let host = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Ok(entries) =
                    std::fs::read_dir(session::session_dir().join(&source).join("controls"))
                {
                    for entry in entries.flatten() {
                        if !entry
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".request.json")
                        {
                            continue;
                        }
                        let request: Value =
                            serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap())
                                .unwrap();
                        assert_eq!(request["ui"]["command"], "open");

                        session::write_closed_tombstone(&source).unwrap();

                        std::thread::sleep(std::time::Duration::from_secs(32));
                        session::write_session(
                            &target,
                            "model.json",
                            expected_model.as_str().unwrap(),
                        )
                        .unwrap();
                        session::write_session(
                            &target,
                            "heartbeat.json",
                            &json!({
                                "updated_ms":session::now_ms(),"generation":1,
                                "session_id":target,"session_mode":"read_only_snapshot"
                            })
                            .to_string(),
                        )
                        .unwrap();
                        let retained = entry.path().is_file()
                            && request["expires_ms"].as_u64().unwrap() >= session::now_ms();
                        if retained {
                            session::write_session(
                                &source,
                                &format!(
                                    "controls/{}.result.json",
                                    request["id"].as_str().unwrap()
                                ),
                                &json!({"request_id":request["id"],"session_id":source,
                                    "status":"applied","active_session_id":target,"completed":true})
                                .to_string(),
                            )
                            .unwrap();
                        }
                        return retained;
                    }
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        let reply = server
            .call_tool(
                "cad_interface",
                json!({
                    "action":"file","command":"open","path":"C:/fixtures/slow-replacement.limo"
                }),
            )
            .unwrap();
        let retained = host.join().unwrap();

        let controls_empty = std::fs::read_dir(dir.join(&original).join("controls"))
            .unwrap()
            .next()
            .is_none();
        let loaded_model = server.manager.export_project_model().unwrap();
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(
            reply["status"], "applied",
            "Completed slow Open lost its acknowledgement: {reply}"
        );
        assert!(
            retained,
            "The native reply must remain eligible after slow reconstruction"
        );
        assert_eq!(reply["session_id"], original);
        assert_eq!(reply["active_session_id"], replacement);
        assert_eq!(reply["attached_session_id"], replacement);
        assert_eq!(loaded_model, model.as_str().unwrap());
        assert!(server.manager.solid_scene().bodies.is_empty());
        assert!(
            controls_empty,
            "Completed request and result files must be removed"
        );
    }

    #[test]
    fn applied_ui_receipt_survives_failed_attachment_and_blocks_stale_model_commands() {
        fn acknowledge(source: String, result: Value) -> std::thread::JoinHandle<()> {
            std::thread::spawn(move || {
                let controls = session::session_dir().join(&source).join("controls");
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                loop {
                    if let Ok(entries) = std::fs::read_dir(&controls) {
                        for entry in entries.flatten() {
                            if !entry
                                .file_name()
                                .to_string_lossy()
                                .ends_with(".request.json")
                            {
                                continue;
                            }
                            let request: Value = serde_json::from_str(
                                &std::fs::read_to_string(entry.path()).unwrap(),
                            )
                            .unwrap();
                            session::write_session(
                                &source,
                                &format!(
                                    "controls/{}.result.json",
                                    request["id"].as_str().unwrap()
                                ),
                                &result.to_string(),
                            )
                            .unwrap();
                            return;
                        }
                    }
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
        }
        let _guard = session::env_lock();
        let original = session::test_session_uuid();
        let replacement = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-attachment-failure-{original}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (_, original_model) = write_box_session(&original);
        session::write_session(&replacement, "model.json", "invalid model").unwrap();
        session::write_session(&replacement,"heartbeat.json",&json!({"updated_ms":session::now_ms(),"interface_version":1,"generation":2,"model_generation":2}).to_string()).unwrap();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":original}))
            .unwrap();
        let worker = acknowledge(
            original.clone(),
            json!({"status":"applied","active_session_id":replacement,"value":{"opened":true}}),
        );
        let receipt = server
            .call_tool(
                "cad_interface",
                json!({"action":"file","command":"open","path":"replacement.limo"}),
            )
            .unwrap();
        worker.join().unwrap();
        assert_eq!(receipt["status"], "applied");
        assert_eq!(receipt["value"]["opened"], true);
        assert_eq!(receipt["active_session_id"], replacement);
        assert_eq!(receipt["attached"], false);
        assert_eq!(receipt["model_commands_blocked"], true);
        assert!(receipt["snapshot_error"].is_string());
        assert_eq!(
            server.attached_document_id.as_deref(),
            Some(original.as_str())
        );
        for (name, args) in [
            ("cad_document", json!({})),
            ("solid_scene", json!({})),
            ("cad_project_model", json!({})),
            (
                "cad_set_document_name",
                json!({"name":"Must not change the previous document"}),
            ),
            (
                "cad_submit",
                json!({"name":"cad_set_document_name","arguments":{"name":"Must not submit"},"base_generation":2}),
            ),
            (
                "cad_interface",
                json!({"action":"execute","group":interface::group_for("cad_document").unwrap(),"operation":"cad_document","arguments":{}}),
            ),
            (
                "cad_interface",
                json!({"action":"script","session_id":replacement,"source":"Must not parse or run"}),
            ),
        ] {
            let error: Value =
                serde_json::from_str(&server.call_tool(name, args).unwrap_err()).unwrap();
            assert_eq!(error["code"], "attachment_unavailable", "{name}");
            assert_eq!(error["session_id"], replacement, "{name}");
        }
        assert_eq!(
            server.manager.export_project_model().unwrap(),
            original_model
        );
        assert!(session::pending_inbox_seqs(&original).unwrap().is_empty());
        assert!(session::pending_inbox_seqs(&replacement)
            .unwrap()
            .is_empty());
        let worker = acknowledge(
            replacement.clone(),
            json!({"status":"applied","active_session_id":replacement,"ui":{"surfaces":[]}}),
        );
        let inspected = server
            .call_tool("cad_interface", json!({"action":"inspect"}))
            .unwrap();
        worker.join().unwrap();
        assert_eq!(inspected["status"], "applied");
        assert_eq!(inspected["attached"], false);
        let status = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(status["session_id"], replacement);
        assert_eq!(status["attached"], false);
        assert_eq!(status["retained_snapshot_session_id"], original);
        assert!(server.call_tool("cad_refresh", json!({})).is_err());
        assert_eq!(
            server.manager.export_project_model().unwrap(),
            original_model
        );
        let mut recovered = CadServer::new().unwrap();
        recovered
            .call_tool(
                "cad_set_document_name",
                json!({"name":"Recovered active document"}),
            )
            .unwrap();
        let model = recovered.manager.export_project_model().unwrap();
        session::write_session(&replacement, "model.json", &model).unwrap();
        let refreshed = server.call_tool("cad_refresh", json!({})).unwrap();
        assert_eq!(refreshed["refreshed"], true);
        assert_eq!(refreshed["session_id"], replacement);
        assert!(server.failed_attachment.is_none());
        assert_eq!(
            server.call_tool("cad_document", json!({})).unwrap()["name"],
            "Recovered active document"
        );
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ready_launch_receipt_keeps_identity_without_a_headless_fallback() {
        let _guard = session::env_lock();
        let target = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-ready-attachment-{target}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        session::write_session(&target, "model.json", "invalid model").unwrap();
        session::write_session(
            &target,
            "heartbeat.json",
            &json!({"updated_ms":session::now_ms(),"interface_version":1,"generation":1})
                .to_string(),
        )
        .unwrap();
        let mut server = CadServer::new().unwrap();
        server.desktop_binding = Some(DesktopBinding {
            process_id: 1234,
            initial_selection_pending: false,
        });
        let mut receipt =
            json!({"status":"ready","pid":1234,"session_id":target,"window_id":"new-window"});
        server
            .follow_interface_attachment(
                &mut receipt,
                target.clone(),
                SnapshotRefresh::DeferDuringScript,
            )
            .unwrap();
        assert_eq!(receipt["status"], "ready");
        assert_eq!(receipt["pid"], 1234);
        assert_eq!(receipt["session_id"], target);
        assert_eq!(receipt["window_id"], "new-window");
        assert_eq!(receipt["attached"], false);
        assert_eq!(receipt["model_commands_blocked"], true);
        assert!(server.attached_document_id.is_none());
        assert!(server
            .call_tool("cad_document", json!({}))
            .unwrap_err()
            .contains("attachment_unavailable"));
        let failure = server.call_tool("cad_refresh", json!({})).unwrap_err();
        assert!(
            !failure.contains("no selected document"),
            "Refresh must retain the acknowledged target for recovery"
        );
        let recovered = CadServer::new()
            .unwrap()
            .manager
            .export_project_model()
            .unwrap();
        session::write_session(&target, "model.json", &recovered).unwrap();
        assert_eq!(
            server.call_tool("cad_refresh", json!({})).unwrap()["session_id"],
            target
        );
        assert_eq!(
            server.attached_document_id.as_deref(),
            Some(target.as_str())
        );
        assert!(server.failed_attachment.is_none());
        assert!(server.call_tool("cad_document", json!({})).is_ok());
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_snapshot_refresh_preserves_applied_receipts_and_requires_recovery() {
        let _guard = session::env_lock();
        for awaiting_receipt in [false, true] {
            let target = session::test_session_uuid();
            let dir = std::env::temp_dir().join(format!("limo-cad-refresh-failure-{target}"));
            std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
            let (_, model) = write_box_session(&target);
            let mut server = CadServer::new().unwrap();
            server
                .call_tool("cad_attach", json!({"session_id":target}))
                .unwrap();
            session::write_session(&target, "model.json", "invalid published model").unwrap();
            session::write_session(&target,"heartbeat.json",&json!({"updated_ms":session::now_ms(),"generation":2,"published_generation":2,"model_generation":2}).to_string()).unwrap();
            if awaiting_receipt {
                let archived = json!({"name":"cad_set_document_name","base_generation":1});
                session::write_session(&target, "inbox/applied/1.json", &archived.to_string())
                    .unwrap();
                for _ in 0..2 {
                    let applied = server
                        .call_tool("cad_await_apply", json!({"seq":1,"timeout_ms":0}))
                        .unwrap();
                    assert_eq!(applied["status"], "applied");
                    assert_eq!(applied["published"], true);
                    assert_eq!(applied["model_published"], true);
                    assert_eq!(applied["refreshed"], false);
                    assert_eq!(applied["model_commands_blocked"], true);
                    assert!(applied["snapshot_error"].is_string());
                }
                assert_eq!(
                    serde_json::from_str::<Value>(
                        &session::read_session_file(&target, "inbox/applied/1.json").unwrap()
                    )
                    .unwrap(),
                    archived
                );
            } else {
                assert!(server.call_tool("cad_refresh", json!({})).is_err());
            }
            assert_eq!(server.manager.export_project_model().unwrap(), model);
            assert!(server
                .call_tool("cad_document", json!({}))
                .unwrap_err()
                .contains("attachment_unavailable"));
            let status = server.call_tool("cad_session_status", json!({})).unwrap();
            assert_eq!(status["attached_generation"], 1);
            assert_eq!(status["stale"], true);
            assert_eq!(status["model_commands_blocked"], true);
            session::write_session(&target, "model.json", &model).unwrap();
            assert_eq!(
                server.call_tool("cad_refresh", json!({})).unwrap()["session_id"],
                target
            );
            assert_eq!(
                server.call_tool("cad_session_status", json!({})).unwrap()["attached_generation"],
                2
            );
            assert!(server.call_tool("cad_document", json!({})).is_ok());
            assert_eq!(server.manager.export_project_model().unwrap(), model);
            assert!(session::pending_inbox_seqs(&target).unwrap().is_empty());
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn explicit_detach_clears_a_failed_target_and_pending_snapshot_refresh() {
        let _guard = session::env_lock();
        let target = session::test_session_uuid();
        let mut server = CadServer::new().unwrap();
        server.failed_attachment = Some(AttachmentFailure {
            session_id: target.clone(),
            error: "Unsupported snapshot".into(),
        });
        server.live_snapshot_dirty = true;
        let detached = server.call_tool("cad_detach", json!({})).unwrap();
        assert_eq!(detached["session_id"], target);
        assert!(server.failed_attachment.is_none());
        assert!(!server.live_snapshot_dirty);
        assert!(server.call_tool("cad_document", json!({})).is_ok());
    }

    #[test]
    fn acknowledged_document_transitions_track_completed_model_fences() {
        let _guard = session::env_lock();
        let first = session::test_session_uuid();
        let second = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-status-transition-{first}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (_, model) = write_box_session(&first);
        session::write_session(&second, "model.json", &model).unwrap();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":first}))
            .unwrap();
        let baseline = server.tool_trace.clone();

        for (active, generation, model_generation) in [
            (first.clone(), 2, 2),
            (first.clone(), 3, 2),
            (second.clone(), 7, 7),
        ] {
            let request_session = server.attached_document_id.clone().unwrap();
            let target = active.clone();
            let worker =
                std::thread::spawn(move || {
                    let controls = session::session_dir()
                        .join(&request_session)
                        .join("controls");
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    loop {
                        if let Ok(entries) = std::fs::read_dir(&controls) {
                            for entry in entries.flatten() {
                                if !entry
                                    .file_name()
                                    .to_string_lossy()
                                    .ends_with(".request.json")
                                {
                                    continue;
                                }
                                let request: Value = serde_json::from_str(
                                    &std::fs::read_to_string(entry.path()).unwrap(),
                                )
                                .unwrap();
                                session::write_session(&target, "heartbeat.json", &json!({
                                "updated_ms":session::now_ms(), "interface_version":1,
                                "generation":generation, "published_generation":generation,
                                "model_generation":model_generation,
                                "active_sketch_generation": if generation != model_generation {
                                    Some(generation)
                                } else { None },
                            }).to_string()).unwrap();
                                session::write_session(
                                    &request_session,
                                    &format!(
                                        "controls/{}.result.json",
                                        request["id"].as_str().unwrap()
                                    ),
                                    &json!({"status":"applied","active_session_id":target})
                                        .to_string(),
                                )
                                .unwrap();
                                std::fs::remove_file(entry.path()).unwrap();
                                return;
                            }
                        }
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                });
            let result = server
                .call_tool(
                    "cad_interface",
                    json!({
                        "action":"file", "command":"new",
                    }),
                )
                .unwrap();
            worker.join().unwrap();
            assert_eq!(result["attached_session_id"], active);
            let status = server
                .call_tool(
                    "cad_interface",
                    json!({
                        "action":"execute", "group":"document/session",
                        "operation":"cad_session_status", "arguments":{},
                    }),
                )
                .unwrap();
            assert_eq!(status["session_id"], active);
            assert_eq!(status["session_mode"], "live");
            assert_eq!(status["heartbeat"]["interface_version"], 1);
            assert_eq!(status["attached_generation"], model_generation);
            assert_eq!(status["generation"], generation);
            assert_eq!(status["stale"], generation != model_generation);
            assert_eq!(
                server.tool_trace.clone(),
                baseline,
                "observing status and UI controls must not add replay operations"
            );
        }

        session::write_session(&second, "model.json", "invalid model").unwrap();
        session::write_session(
            &second,
            "heartbeat.json",
            &json!({
                "generation":8, "model_generation":8,
            })
            .to_string(),
        )
        .unwrap();
        assert!(server.call_tool("cad_refresh", json!({})).is_err());
        let status = server.call_tool("cad_session_status", json!({})).unwrap();
        assert_eq!(status["attached_generation"], 7);
        assert_eq!(status["generation"], 8);
        assert_eq!(status["stale"], true);
        assert_eq!(server.manager.solid_scene().bodies.len(), 1);
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn live_script_completion_reports_uncaptained_steps_only_after_checks_pass() {
        let _guard = session::env_lock();
        for (mode, checks_pass) in [
            ("present", true),
            ("fast", true),
            ("present", false),
            ("fast", false),
        ] {
            let target = session::test_session_uuid();
            let dir = std::env::temp_dir().join(format!("limo-cad-script-completion-{target}"));
            std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
            let mut server = CadServer::new().unwrap();
            let blank = server.call_tool("cad_project_model", json!({})).unwrap();
            session::write_session(&target, "model.json", blank.as_str().unwrap()).unwrap();
            session::write_session(
                &target,
                "heartbeat.json",
                &json!({
                    "updated_ms":session::now_ms(),"generation":1,"session_id":target,"interface_version":1,
                })
                .to_string(),
            )
            .unwrap();
            let peer = target.clone();

            let host = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut seen = std::collections::HashSet::new();
                let mut controls = Vec::new();
                let mut progress = Vec::new();
                let mut owner = CadServer::new().unwrap();
                loop {
                    if let Some(seq) = session::pending_inbox_seqs(&peer).unwrap().first().copied()
                    {
                        let applied = session::apply_inbox_op(&peer, |name, args| {
                            let result = owner.call_tool(name, args)?;
                            session::write_session(
                                &peer,
                                &format!("inbox/results/{seq}.json"),
                                &result.to_string(),
                            )?;
                            let model = owner
                                .manager
                                .export_project_model()
                                .map_err(|error| error.to_string())?;
                            session::publish_applied_snapshot(&peer, &model)?;
                            let mut heartbeat: Value = serde_json::from_str(
                                &session::read_session_file(&peer, "heartbeat.json")?,
                            )
                            .unwrap();
                            heartbeat["interface_version"] = json!(1);
                            session::write_session(
                                &peer,
                                "heartbeat.json",
                                &heartbeat.to_string(),
                            )?;
                            Ok(result)
                        })
                        .unwrap();
                        progress.push(applied.op.script_progress);
                    }
                    if let Ok(entries) =
                        std::fs::read_dir(session::session_dir().join(&peer).join("controls"))
                    {
                        for entry in entries.flatten() {
                            if !entry
                                .file_name()
                                .to_string_lossy()
                                .ends_with(".request.json")
                                || !seen.insert(entry.path())
                            {
                                continue;
                            }
                            let request: Value = serde_json::from_str(
                                &std::fs::read_to_string(entry.path()).unwrap(),
                            )
                            .unwrap();
                            let done = matches!(
                                request["ui"]["command"].as_str(),
                                Some("finish" | "stop")
                            );
                            let reply = if !request["sketch_query"].is_null() {
                                assert_eq!(request["sketch_query"]["method"], "active_sketch");
                                json!({"status":"applied","value":null})
                            } else {
                                controls.push(request.clone());
                                json!({"status":"applied","presentation":{"wait_ms":0}})
                            };
                            session::write_session(
                                &peer,
                                &format!(
                                    "controls/{}.result.json",
                                    request["id"].as_str().unwrap()
                                ),
                                &reply.to_string(),
                            )
                            .unwrap();
                            if done {
                                return (
                                    controls,
                                    progress,
                                    owner.manager.export_project_model().unwrap(),
                                );
                            }
                        }
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Script completion control did not arrive"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
            });
            let source = json!({"version":1,"name":"Sparse captions","steps":[
                {"note":"The only caption","chapter":"Beginning"},
                {"let":{"dimension":12}},
                {"call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":"Progress model"}}},
                {"assert":{"$ref":"dimension"},"equals":12},
                {"view":"isometric","fit":true},
                {"call":{"group":interface::group_for("drawing_create_sheet").unwrap(),"operation":"drawing_create_sheet",
                    "arguments":{"name":"Drawing phase","format":"a4","orientation":"landscape"}}}
            ],"checks":[{"assert":{"$ref":"dimension"},"equals":if checks_pass {12} else {13}}]})
            .to_string();
            let result = server.call_tool(
                "cad_interface",
                json!({
                    "action":"script","source":source,"session_id":target,"mode":mode,
                }),
            );
            let (controls, progress, live_model) = host.join().unwrap();
            assert_eq!(
                progress,
                [2, 5]
                    .map(|steps_completed| {
                        Some(limo_cad_script::RunProgress {
                            steps_completed,
                            step_count: 6,
                        })
                    })
                    .to_vec(),
                "Both modes report operations between sparse chapter notes"
            );
            assert!(
                server.script_progress.is_none(),
                "Progress context must end with the runner"
            );
            assert_eq!(
                server.manager.export_project_model().unwrap(),
                live_model,
                "Progress transport must preserve the actual completed model and drawing"
            );
            assert_eq!(controls[0]["ui"]["command"], "configure");
            assert_eq!(controls[0]["ui"]["chapter"], "");
            assert_eq!(controls[0]["ui"]["step_index"], 0);
            assert_eq!(controls[0]["ui"]["step_count"], 6);
            let final_ui = &controls.last().unwrap()["ui"];
            if checks_pass {
                let report = result.unwrap();
                assert_eq!(report["steps_completed"], 6);
                assert_eq!(report["checks_completed"], 1);
                assert_eq!(final_ui["command"], "finish");
                assert_eq!(final_ui["step_index"], report["steps_completed"]);
                assert_eq!(final_ui["step_count"], 6);
            } else {
                assert!(result.unwrap_err().contains("Assertion failed"));
                assert_eq!(final_ui["command"], "stop");
                assert!(
                    final_ui["step_index"].is_null(),
                    "A failed check must not claim completion"
                );
            }
            if mode == "fast" {
                assert_eq!(
                    controls.len(),
                    2,
                    "Maximum rate needs only configure and completion controls"
                );
            } else {
                assert_eq!(controls[1]["ui"]["step_index"], 1);
                assert_eq!(controls[controls.len() - 2]["view"], "isometric");
            }
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn script_configuration_cannot_retarget_a_different_active_document() {
        let _guard = session::env_lock();
        let target = session::test_session_uuid();
        let other = "fdec1d2c-6dea-4e73-a20e-0ae5a9a90252";
        let dir = std::env::temp_dir().join(format!("limo-cad-script-tab-race-{target}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let mut donor = CadServer::new().unwrap();
        let blank = donor.call_tool("cad_project_model", json!({})).unwrap();
        session::write_session(&target, "model.json", blank.as_str().unwrap()).unwrap();
        session::write_session(
            &target,
            "heartbeat.json",
            &json!({
                "updated_ms":session::now_ms(),"generation":1,"session_id":target,
            })
            .to_string(),
        )
        .unwrap();
        let (_, preserved) = write_box_session(other);
        let peer = target.clone();
        let host = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut seen = std::collections::HashSet::new();
            loop {
                if let Ok(entries) =
                    std::fs::read_dir(session::session_dir().join(&peer).join("controls"))
                {
                    for entry in entries.flatten() {
                        if !entry
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".request.json")
                            || !seen.insert(entry.path())
                        {
                            continue;
                        }
                        let request: Value =
                            serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap())
                                .unwrap();
                        let configuring = request["ui"]["command"] == "configure";
                        let reply = if configuring {
                            json!({"status":"applied","active_session_id":other})
                        } else {
                            assert_eq!(request["sketch_query"]["method"], "active_sketch");
                            json!({"status":"applied","value":null})
                        };
                        session::write_session(
                            &peer,
                            &format!("controls/{}.result.json", request["id"].as_str().unwrap()),
                            &reply.to_string(),
                        )
                        .unwrap();
                        if configuring {
                            return;
                        }
                    }
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let source = json!({"version":1,"name":"Stay on the selected blank tab","steps":[
            {"call":{"group":interface::group_for("cad_set_document_name").unwrap(),
                "operation":"cad_set_document_name","arguments":{"name":"Must not overwrite"}}}
        ]})
        .to_string();
        let mut server = CadServer::new().unwrap();
        let error = server
            .call_tool(
                "cad_interface",
                json!({"action":"script","source":source,
            "session_id":target,"mode":"present"}),
            )
            .unwrap_err();
        host.join().unwrap();
        assert!(error.contains("Active document changed"), "{error}");
        assert_eq!(
            server.attached_document_id.as_deref(),
            Some(target.as_str())
        );
        assert!(
            !server.script_running,
            "failed startup must release the run guard"
        );
        assert!(session::pending_inbox_seqs(&target).unwrap().is_empty());
        assert!(session::pending_inbox_seqs(other).unwrap().is_empty());
        assert_eq!(session::require_model_json(other).unwrap(), preserved);
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn grouped_interface_rejects_an_old_desktop_before_submitting() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-old-interface-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_box_session(&unique);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":unique}))
            .unwrap();
        let error = server
            .call_tool(
                "cad_interface",
                json!({"action":"execute","group":"sketch/draw",
            "operation":"sketch_begin","arguments":{"plane":{"type":"origin_plane","plane":"xy"}}}),
            )
            .unwrap_err();
        assert!(error.contains("nothing submitted"));
        assert!(session::pending_inbox_seqs(&unique).unwrap().is_empty());
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn unreplied_control_request(
        entry: &std::fs::DirEntry,
        replied: &std::collections::HashSet<String>,
    ) -> Option<Value> {
        let filename = entry.file_name();
        let request_id = filename.to_str()?.strip_suffix(".request.json")?;
        if replied.contains(request_id) {
            return None;
        }
        let source = match std::fs::read_to_string(entry.path()) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => panic!("Read mock control request: {error}"),
        };
        let request: Value = serde_json::from_str(&source).unwrap();
        assert_eq!(request["id"].as_str(), Some(request_id));
        Some(request)
    }

    #[test]
    fn mock_control_poll_retains_pending_requests_and_tolerates_acknowledgment_cleanup() {
        let dir = std::env::temp_dir().join(format!(
            "limo-cad-control-poll-{}",
            session::test_session_uuid()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("request-1.request.json");
        let request = json!({"id":"request-1","ui":{"action":"open_recipe"}});
        std::fs::write(&path, request.to_string()).unwrap();
        let entry = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap();
        let pending = std::collections::HashSet::new();
        assert_eq!(unreplied_control_request(&entry, &pending), Some(request));
        let replied = std::collections::HashSet::from(["request-1".to_owned()]);
        assert!(unreplied_control_request(&entry, &replied).is_none());
        std::fs::remove_file(&path).unwrap();
        assert!(unreplied_control_request(&entry, &replied).is_none());
        assert!(unreplied_control_request(&entry, &pending).is_none());
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn open_recipe_receipt_never_attaches_or_rehydrates_the_model() {
        let _guard = session::env_lock();
        let id = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-open-recipe-control-{id}"));
        let previous = std::env::var_os("LIMO_CAD_SESSION_DIR");
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        session::write_session(
            &id,
            "heartbeat.json",
            &json!({"updated_ms":session::now_ms(),"generation":9}).to_string(),
        )
        .unwrap();
        session::write_session(
            &id,
            "model.json",
            "This model must never be read while opening source",
        )
        .unwrap();
        let target = id.clone();
        let receiver = std::thread::spawn(move || {
            let controls = session::session_dir().join(&target).join("controls");
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut replied = std::collections::HashSet::new();
            while std::time::Instant::now() < deadline {
                if let Ok(entries) = std::fs::read_dir(&controls) {
                    for entry in entries.flatten() {
                        let Some(request) = unreplied_control_request(&entry, &replied) else {
                            continue;
                        };
                        let request_id = request["id"].as_str().unwrap();
                        if !replied.insert(request_id.to_owned()) {
                            continue;
                        }
                        let response = json!({"status":"applied", "active_session_id":target,
                            "recipe":{"status":"queued","recipe":"garden-bench"}});
                        session::write_session(
                            &target,
                            &format!("controls/{request_id}.result.json"),
                            &response.to_string(),
                        )
                        .unwrap();
                        if replied.len() == 2 {
                            return;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("The MCP source-open calls did not reach live control");
        });
        for attached in [None, Some(id.clone())] {
            let mut server = CadServer::new().unwrap();
            let model = server.manager.export_project_model().unwrap();
            server.attached_document_id = attached.clone();
            server.attached_generation = Some(7);
            server.live_snapshot_dirty = true;
            let result = server
                .call_tool(
                    "cad_interface",
                    json!({"action":"open_recipe","recipe":"garden-bench","session_id":id}),
                )
                .unwrap();
            assert_eq!(result["recipe"]["status"], "queued");
            assert_eq!(server.attached_document_id, attached);
            assert_eq!(server.attached_generation, Some(7));
            assert!(server.live_snapshot_dirty);
            assert_eq!(server.manager.export_project_model().unwrap(), model);
        }
        receiver.join().unwrap();
        if let Some(value) = previous {
            std::env::set_var("LIMO_CAD_SESSION_DIR", value);
        } else {
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cam_tools_share_the_product_catalog_and_persist_in_the_project() {
        let expected = [
            ("cam_get_document", "cam/setup"),
            ("cam_set_document", "cam/setup"),
            ("cam_toolpath_statuses", "cam/toolpaths"),
            ("cam_regenerate_operation", "cam/toolpaths"),
            ("cam_regenerate_setup", "cam/toolpaths"),
            ("cam_plan_setup", "cam/toolpaths"),
            ("cam_simulate_setup", "cam-simulate/simulation"),
            ("cam_simulate_gcode", "cam-simulate/simulation"),
            ("cam_post_events", "cam-output/advanced"),
            ("cam_post_setup", "cam-output/output"),
        ];
        let tools = tool_specs();
        for (name, group) in expected {
            assert_eq!(tools.iter().filter(|tool| tool.name == name).count(), 1);
            assert_eq!(interface::group_for(name), Some(group));
        }
        assert!(is_read_safe_while_attached("cam_get_document"));
        assert!(is_read_safe_while_attached("cam_toolpath_statuses"));
        assert!(!records_in_script("cam_get_document"));
        assert!(!records_in_script("cam_toolpath_statuses"));
        let mut server = CadServer::new().unwrap();
        let mut cam = server.call_tool("cam_get_document", json!({})).unwrap();
        cam["units"] = json!("inches");
        server
            .call_tool(
                "cad_interface",
                json!({
                    "action":"execute", "group":"cam/setup", "operation":"cam_set_document",
                    "arguments":cam
                }),
            )
            .unwrap();
        assert_eq!(
            server.call_tool("cam_get_document", json!({})).unwrap()["units"],
            "inches"
        );
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        let model: Value = serde_json::from_str(model.as_str().unwrap()).unwrap();
        assert_eq!(model["cam"]["units"], "inches");
        assert_eq!(
            model["schema_version"],
            limo_cad_sketch::PROJECT_SCHEMA_VERSION
        );
        assert_eq!(model["views"], json!([]));
    }

    #[test]
    fn live_ui_has_one_catalog_surface_and_shared_mutation_metadata() {
        let catalog = full_tool_catalog();
        let catalog = catalog.as_array().unwrap();
        assert_eq!(
            catalog
                .iter()
                .filter(|t| t["name"] == "cad_interface")
                .count(),
            1
        );
        for retired in ["cad_view", "cad_launch", "cad_ui"] {
            assert!(!catalog.iter().any(|t| t["name"] == retired));
        }
        for tool in catalog {
            let name = tool["name"].as_str().unwrap();
            assert_eq!(tool["mutates"], changes_model(name));
            assert!(
                interface::group_for(name).is_some(),
                "ungrouped operation: {name}"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for group in interface::groups() {
            for operation in group["operations"].as_array().unwrap() {
                let name = operation.as_str().unwrap();
                assert!(seen.insert(name), "duplicate operation grouping: {name}");
                assert!(
                    catalog.iter().any(|tool| tool["name"] == name),
                    "stale catalog operation: {name}"
                );
            }
        }
    }

    #[test]
    fn every_shared_mutate_is_accepted_by_cad_submit_classifier() {
        for spec in limo_cad_mcp_mutate::mutate_specs() {
            assert!(
                is_modeling_mutate(spec.name),
                "{} must classify as modeling mutate",
                spec.name
            );
            assert!(
                !is_read_safe_while_attached(spec.name),
                "{} must not be read-safe while attached",
                spec.name
            );
        }
    }

    #[test]
    fn leftover_and_native_apply_share_error_class_and_archive() {
        let leftover = include_str!("session.rs");
        let native = include_str!("../../desktop/src/session_bridge.rs");
        for (label, source) in [("leftover", leftover), ("native", native)] {
            assert!(
                source.contains("\"code\": \"generation_conflict\"")
                    && source.contains("\"writeback\": false")
                    && source.contains("\"session_mode\": \"ui_owned_apply\""),
                "{label} generation_conflict must be the structured class"
            );
            assert!(
                source.contains("unsupported inbox mutate"),
                "{label} unsupported mutate must share the class string"
            );
            assert!(
                source.contains("inbox/failed") || source.contains("inbox/failed/"),
                "{label} must dead-letter into inbox/failed"
            );
        }
        let leftover_apply = leftover
            .find("pub fn apply_inbox_op")
            .expect("leftover apply_inbox_op");
        let leftover_apply_end = leftover[leftover_apply..]
            .find("pub fn publish_applied_snapshot")
            .expect("end leftover apply");
        let leftover_fn = &leftover[leftover_apply..leftover_apply + leftover_apply_end];
        assert!(
            leftover_fn.contains("dead_letter_inbox_op")
                && leftover_fn.matches("dead_letter_inbox_op").count() >= 4,
            "leftover must dead-letter missing heartbeat, mismatch, unsupported, and host fail"
        );
        let native_apply = native
            .find("fn apply_or_reject_one_inbox_op_with_editor_guards(")
            .expect("native inbox apply implementation");
        let native_apply_end = native[native_apply..]
            .find("\n}")
            .expect("closing brace of native apply");
        let native_fn = &native[native_apply..native_apply + native_apply_end];
        assert!(
            native_fn.contains("project.engine_revision")
                && !native_fn.contains("read_heartbeat_generation")
                && !native_fn.contains("heartbeat_meta"),
            "native apply must lock on in-memory engine_revision, not heartbeat.json age/file"
        );
    }

    fn planar_connector_from_body(body: &Value) -> Value {
        let face = body["faces"]
            .as_array()
            .expect("body faces")
            .iter()
            .find(|face| face.get("plane").is_some() && !face["plane"].is_null())
            .expect("planar face");
        let plane = &face["plane"];
        json!({
            "body_id": body["id"],
            "face_id": face["id"],
            "face_key": face["key"],
            "kind": "planar_face",
            "frame": {
                "origin": plane["origin"],
                "primary_axis": plane["normal"],
                "secondary_axis": plane["u"]
            }
        })
    }

    fn extrude_offset_box(server: &mut CadServer, sketch_name: &str, x0: f64, x1: f64) -> Value {
        server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        server
            .call_tool(
                "sketch_add_rectangle",
                json!({
                    "mode": "two_point",
                    "p1": {"x": x0, "y": -5.0},
                    "p2": {"x": x1, "y": 5.0},
                    "ctrl_held": false
                }),
            )
            .unwrap();
        server.call_tool("sketch_finish", json!({})).unwrap();
        server
            .call_tool(
                "solid_extrude",
                json!({
                    "sketch_name": sketch_name,
                    "profile_indices": [0],
                    "operation": "new_body",
                    "extent": {"type": "distance", "distance": 8.0},
                    "taper_angle_deg": 0.0,
                    "flip": false,
                    "target_body_ids": []
                }),
            )
            .unwrap()
    }

    #[test]
    fn gear_relation_tools_drive_persist_restore_and_delete_native_coordinates() {
        let mut server = CadServer::new().unwrap();
        extrude_offset_box(&mut server, "Sketch1", -24.0, -14.0);
        extrude_offset_box(&mut server, "Sketch2", -5.0, 5.0);
        let third = extrude_offset_box(&mut server, "Sketch3", 14.0, 24.0);
        let bodies = third["scene"]["bodies"].as_array().unwrap();
        let mut ids = Vec::new();
        for index in 1..=2 {
            let joint = server.call_tool("assembly_create_joint", json!({
                "name": format!("Shaft {index}"), "kind":"revolute", "grounded_body_id":bodies[0]["id"],
                "connector_a":planar_connector_from_body(&bodies[0]),
                "connector_b":planar_connector_from_body(&bodies[index]), "flipped":true
            })).unwrap();
            ids.push(joint["id"].clone());
        }
        let mut relation = server.call_tool("assembly_create_gear_relation", json!({
            "name":"Turbine pair", "joint_a":ids[0], "joint_b":ids[1], "teeth_a":80, "teeth_b":20, "phase_deg":12.0
        })).unwrap();
        relation.as_object_mut().unwrap().remove("_disclosure");
        server
            .call_tool(
                "assembly_set_joint_motion",
                json!({"joint_id":ids[0],"angle_offset_deg":810.0,"linear_offset_mm":0.0}),
            )
            .unwrap();
        let document = server.call_tool("assembly_document", json!({})).unwrap();
        assert_eq!(document["joints"][1]["angle_offset_deg"], -3228.0);
        assert_eq!(document["gear_relations"][0], relation);
        assert_eq!(
            server.call_tool("assembly_solution", json!({})).unwrap()["solved"],
            true
        );
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool(
                "cad_load_project_model",
                json!({"model_json":model.as_str().unwrap()}),
            )
            .unwrap();
        assert_eq!(
            restored.call_tool("assembly_document", json!({})).unwrap(),
            document
        );
        let mut changed = relation.clone();
        changed["phase_deg"] = json!(0.0);
        restored
            .call_tool("assembly_update_gear_relation", changed)
            .unwrap();
        assert_eq!(
            restored.call_tool("assembly_document", json!({})).unwrap()["joints"][1]
                ["angle_offset_deg"],
            -3240.0
        );
        restored
            .call_tool(
                "assembly_delete_gear_relation",
                json!({"relation_id":relation["id"]}),
            )
            .unwrap();
        assert_eq!(
            restored.call_tool("assembly_document", json!({})).unwrap()["gear_relations"],
            json!([])
        );
    }

    #[test]
    fn contact_tools_preserve_instances_validate_and_roundtrip() {
        let mut server = CadServer::new().unwrap();
        extrude_offset_box(&mut server, "Sketch1", -12., -2.);
        extrude_offset_box(&mut server, "Sketch2", 2., 12.);
        let source = server.call_tool("solid_scene", json!({})).unwrap();
        let poses = server.call_tool("assembly_solution", json!({})).unwrap()
            ["instance_body_poses"]
            .clone();
        let args = json!({"name":"Travel stop","occurrence_a":poses[0]["occurrence_id"],"body_a":poses[0]["body_id"],"occurrence_b":poses[1]["occurrence_id"],"body_b":poses[1]["body_id"],"clearance_mm":0.3,"stop_motion":true});
        let mut invalid = args.clone();
        invalid["clearance_mm"] = json!(-1.);
        assert!(server
            .call_tool("assembly_create_contact_set", invalid)
            .is_err());
        server
            .call_tool("assembly_create_contact_set", args)
            .unwrap();
        let mut contact =
            server.call_tool("assembly_document", json!({})).unwrap()["contact_sets"][0].clone();
        contact["enabled"] = json!(false);
        contact["name"] = json!("Edited stop");
        server
            .call_tool("assembly_update_contact_set", contact.clone())
            .unwrap();
        assert_eq!(
            server.call_tool("assembly_document", json!({})).unwrap()["contact_sets"][0],
            contact
        );
        assert_eq!(server.call_tool("solid_scene", json!({})).unwrap(), source);
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool(
                "cad_load_project_model",
                json!({"model_json":model.as_str().unwrap()}),
            )
            .unwrap();
        assert_eq!(
            restored.call_tool("assembly_document", json!({})).unwrap()["contact_sets"][0],
            contact
        );
        restored
            .call_tool(
                "assembly_delete_contact_set",
                json!({"contact_id":contact["id"]}),
            )
            .unwrap();
        assert_eq!(
            restored.call_tool("assembly_document", json!({})).unwrap()["contact_sets"],
            json!([])
        );
    }

    #[test]
    fn motion_studio_tools_roundtrip_typed_drivers_positions_and_read_only_paths() {
        let mut server = CadServer::new().unwrap();
        extrude_offset_box(&mut server, "Sketch1", -12., -2.);
        let scene = extrude_offset_box(&mut server, "Sketch2", 2., 12.);
        let bodies = scene["scene"]["bodies"].as_array().unwrap();
        let joint=server.call_tool("assembly_create_joint",json!({"name":"Slide","kind":"slider","grounded_body_id":bodies[0]["id"],"connector_a":planar_connector_from_body(&bodies[0]),"connector_b":planar_connector_from_body(&bodies[1])})).unwrap();
        let mut study = server
            .call_tool(
                "assembly_create_motion_study",
                json!({"name":"Travel","duration_seconds":2.}),
            )
            .unwrap();
        study["drivers"] = json!([{"id":1,"name":"Motor","joint_id":joint["id"],"coordinate":"primary_linear","enabled":true,"law":{"kind":"motor","initial_value":0.,"velocity_per_second":4.,"acceleration_per_second2":2.}}]);
        study["next_driver_id"] = json!(2);
        server
            .call_tool("assembly_update_motion_study", study.clone())
            .unwrap();
        let before = server.call_tool("cad_project_model", json!({})).unwrap();
        let evaluation = server
            .call_tool(
                "assembly_evaluate_motion_study",
                json!({"study_id":study["id"],"time_seconds":1.}),
            )
            .unwrap();
        assert_eq!(
            evaluation["sample"]["joint_motions"][0]["linear_offset_mm"],
            5.
        );
        let mut sample = server
            .call_tool(
                "assembly_sample_motion_study",
                json!({"study_id":study["id"],"time_seconds":1.}),
            )
            .unwrap();
        sample.as_object_mut().unwrap().remove("_disclosure");
        assert_eq!(sample, evaluation["sample"]);
        let csv = server
            .call_tool(
                "assembly_export_motion_path_csv",
                json!({"study_id":study["id"],"sample_rate_hz":10,"occurrence_ids":[]}),
            )
            .unwrap();
        assert!(csv.as_str().unwrap().lines().count() > 10);
        assert_eq!(
            server.call_tool("cad_project_model", json!({})).unwrap(),
            before
        );
        let mut position = server
            .call_tool(
                "assembly_create_position",
                json!({"name":"Middle","motions":evaluation["sample"]["joint_motions"]}),
            )
            .unwrap();
        position["name"] = json!("Captured middle");
        server
            .call_tool("assembly_update_position", position.clone())
            .unwrap();
        server
            .call_tool(
                "assembly_apply_position",
                json!({"position_id":position["id"]}),
            )
            .unwrap();
        assert_eq!(
            server.call_tool("assembly_document", json!({})).unwrap()["joints"][0]
                ["linear_offset_mm"],
            5.
        );
        let model = server.call_tool("cad_project_model", json!({})).unwrap();
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool(
                "cad_load_project_model",
                json!({"model_json":model.as_str().unwrap()}),
            )
            .unwrap();
        assert_eq!(
            restored.call_tool("assembly_document", json!({})).unwrap(),
            server.call_tool("assembly_document", json!({})).unwrap()
        );
        restored
            .call_tool(
                "assembly_delete_position",
                json!({"position_id":position["id"]}),
            )
            .unwrap();
        restored
            .call_tool(
                "assembly_delete_motion_study",
                json!({"study_id":study["id"]}),
            )
            .unwrap();
        let empty = restored.call_tool("assembly_document", json!({})).unwrap();
        assert_eq!(empty["positions"], json!([]));
        assert_eq!(empty["motion_studies"], json!([]));
        for op in [
            "assembly_evaluate_motion_study",
            "assembly_sample_motion_study",
            "assembly_export_motion_path_csv",
        ] {
            assert!(is_read_safe_while_attached(op));
            assert!(limo_cad_mcp_mutate::is_live_engine_query(op));
        }
    }

    #[test]
    fn mechanism_tools_share_read_only_preview_and_atomic_coordinate_commit() {
        let mut s = CadServer::new().unwrap();
        extrude_offset_box(&mut s, "Sketch1", -12., -2.);
        let scene = extrude_offset_box(&mut s, "Sketch2", 2., 12.);
        let bodies = scene["scene"]["bodies"].as_array().unwrap();
        let joint=s.call_tool("assembly_create_joint",json!({"name":"Slide","kind":"slider","grounded_body_id":bodies[0]["id"],"connector_a":planar_connector_from_body(&bodies[0]),"connector_b":planar_connector_from_body(&bodies[1])})).unwrap();
        let before = s.call_tool("cad_project_model", json!({})).unwrap();
        let reachable=s.call_tool("assembly_preview_joint_coordinates",json!({"motion":{"joint_id":joint["id"],"angle_offset_deg":0.,"linear_offset_mm":8.}})).unwrap();
        let pose = reachable["body_poses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pose| pose["body_id"] == bodies[1]["id"])
            .unwrap();
        let result=s.call_tool("cad_interface",json!({"action":"execute","group":"assembly/joints","operation":"assembly_preview_mechanism_drag","arguments":{"body_id":bodies[1]["id"],"target_pose":pose,"maximum_iterations":12}})).unwrap();
        assert_eq!(result["solution"]["solved"], true);
        assert_eq!(result["converged"], true, "{result}");
        assert!(
            (result["joint_motions"][0]["linear_offset_mm"]
                .as_f64()
                .unwrap()
                - 8.)
                .abs()
                < 0.02
        );
        assert_eq!(s.call_tool("cad_project_model", json!({})).unwrap(), before);
        s.call_tool("cad_interface",json!({"action":"execute","group":"assembly/joints","operation":"assembly_apply_joint_motions","arguments":{"motions":result["joint_motions"]}})).unwrap();
        assert_ne!(s.call_tool("cad_project_model", json!({})).unwrap(), before);
        assert!(!limo_cad_mcp_mutate::is_inbox_mutate(
            "assembly_preview_mechanism_drag"
        ));
        assert!(limo_cad_mcp_mutate::is_inbox_mutate(
            "assembly_apply_joint_motions"
        ));
    }

    #[test]
    fn swept_inspection_is_exact_read_only_bounded_and_deterministic() {
        let mut server = CadServer::new().unwrap();
        extrude_offset_box(&mut server, "Sketch1", -12., -2.);
        extrude_offset_box(&mut server, "Sketch2", 2., 12.);
        parse_engine_envelope(host::handle(
            &mut server.manager,
            "assembly_create_motion_study",
            r#"{"name":"Stationary pair","duration_seconds":0.1}"#,
        ))
        .unwrap();
        let before = server.call_tool("cad_project_model", json!({})).unwrap();
        let args = json!({"study_id":1,"sample_rate_hz":10,"clearance_threshold_mm":5.,"stop_at_first":true});
        let report = server
            .call_tool("assembly_swept_collision_check", args.clone())
            .unwrap();
        assert_eq!(report["exact"], true);
        assert_eq!(report["sample_count"], 1);
        assert_eq!(report["events"].as_array().unwrap().len(), 1);
        assert_eq!(
            server
                .call_tool("assembly_swept_collision_check", args)
                .unwrap(),
            report
        );
        assert_eq!(
            server.call_tool("cad_project_model", json!({})).unwrap(),
            before
        );
        assert!(is_read_safe_while_attached(
            "assembly_swept_collision_check"
        ));
        assert!(limo_cad_mcp_mutate::is_live_engine_query(
            "assembly_swept_collision_check"
        ));
        for args in [
            json!({"study_id":1,"sample_rate_hz":0}),
            json!({"study_id":1,"clearance_threshold_mm":-1}),
        ] {
            assert!(server
                .call_tool("assembly_swept_collision_check", args)
                .is_err());
        }
        parse_engine_envelope(host::handle(
            &mut server.manager,
            "assembly_create_motion_study",
            r#"{"name":"Enormous duration","duration_seconds":1e30}"#,
        ))
        .unwrap();
        let error = server
            .call_tool("assembly_swept_collision_check", json!({"study_id":2}))
            .unwrap_err();
        assert!(error.contains("100,001"), "{error}");
    }

    #[test]
    fn joint_control_tools_preserve_definition_and_delete_by_id() {
        let mut server = CadServer::new().unwrap();
        extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let bodies = second["scene"]["bodies"].as_array().unwrap();
        let joint = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name":"Hinge", "kind":"revolute", "grounded_body_id":bodies[0]["id"],
                    "connector_a":planar_connector_from_body(&bodies[0]),
                    "connector_b":planar_connector_from_body(&bodies[1]),
                    "limits":{"min":-90,"max":90}
                }),
            )
            .unwrap();
        let id = joint["id"].clone();
        server
            .call_tool(
                "assembly_set_joint_enabled",
                json!({"joint_id":id,"enabled":false}),
            )
            .unwrap();
        let disabled = server.call_tool("assembly_document", json!({})).unwrap();
        assert_eq!(disabled["joints"][0]["enabled"], false);
        assert_eq!(disabled["joints"][0]["connector_a"], joint["connector_a"]);
        server
            .call_tool(
                "assembly_set_joint_enabled",
                json!({"joint_id":id,"enabled":true}),
            )
            .unwrap();
        server
            .call_tool(
                "assembly_set_joint_motion",
                json!({"joint_id":id,"angle_offset_deg":30,"linear_offset_mm":0}),
            )
            .unwrap();
        let moved = server.call_tool("assembly_document", json!({})).unwrap();
        assert_eq!(moved["joints"][0]["angle_offset_deg"], 30.0);
        assert_eq!(moved["joints"][0]["limits"], joint["limits"]);
        server
            .call_tool("assembly_delete_joint", json!({"joint_id":id}))
            .unwrap();
        assert_eq!(
            server.call_tool("assembly_document", json!({})).unwrap()["joints"],
            json!([])
        );
    }

    #[test]
    fn assembly_joint_create_update_query_roundtrip() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let body_b_id = second["scene"]["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|body| body["id"].as_u64().unwrap())
            .max()
            .expect("second body id");
        let scene = server
            .call_tool("solid_scene", json!({}))
            .expect("solid scene");
        let bodies = scene["bodies"].as_array().expect("bodies");
        assert_eq!(bodies.len(), 2, "expected two extruded bodies: {scene}");
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"].as_u64() == Some(body_b_id))
            .cloned()
            .expect("body B");

        let created = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "Hinge1",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(&body_a),
                    "connector_b": planar_connector_from_body(&body_b),
                    "grounded_body_id": body_a["id"],
                    "limits": {"min": -90.0, "max": 90.0}
                }),
            )
            .expect("create joint");
        assert_eq!(created["name"], "Hinge1");
        assert_eq!(created["kind"], "revolute");
        let joint_id = created["id"].as_u64().expect("joint id");

        let document = server
            .call_tool("assembly_document", json!({}))
            .expect("query joints");
        let joints = document["joints"].as_array().expect("joints");
        assert!(
            joints
                .iter()
                .any(|joint| joint["id"].as_u64() == Some(joint_id)
                    && joint["name"] == "Hinge1"
                    && joint["kind"] == "revolute"),
            "assembly_document missing created joint: {document}"
        );

        let mut joint = created.clone();
        if let Some(object) = joint.as_object_mut() {
            object.remove("_disclosure");
            object.insert("name".to_string(), json!("Hinge1Renamed"));
            object.insert("limits".to_string(), json!({"min": -45.0, "max": 45.0}));
        }
        let updated = server
            .call_tool("assembly_update_joint", json!({ "joint": joint }))
            .expect("update joint");
        assert_eq!(updated["name"], "Hinge1Renamed");
        assert_eq!(updated["id"].as_u64(), Some(joint_id));

        let inspect = server
            .call_tool("assembly_document", json!({}))
            .expect("re-query joints");
        let joints = inspect["joints"].as_array().expect("joints after update");
        let found = joints
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .expect("updated joint in document");
        assert_eq!(found["name"], "Hinge1Renamed");
        assert!((found["limits"]["min"].as_f64().unwrap() + 45.0).abs() < 1e-9);
        assert!((found["limits"]["max"].as_f64().unwrap() - 45.0).abs() < 1e-9);
        let solution = server
            .call_tool("assembly_solution", json!({}))
            .expect("assembly solution after joint");
        assert!(
            solution.as_object().map(|o| !o.is_empty()).unwrap_or(false),
            "expected a non-empty assembly solution: {solution}"
        );
    }

    fn write_one_box_session(unique: &str) -> Value {
        let mut donor = CadServer::new().unwrap();
        donor.call_tool("cad_new_project", json!({})).unwrap();
        extrude_offset_box(&mut donor, "Sketch1", -12.0, -2.0);
        let model = donor.call_tool("cad_project_model", json!({})).unwrap();
        let model_json = model
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&model).unwrap());
        session::write_session(unique, "model.json", &model_json).unwrap();
        session::write_session(
            unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();
        donor
            .call_tool("solid_scene", json!({}))
            .expect("donor scene")
    }

    fn write_two_box_session(unique: &str) -> Value {
        let mut donor = CadServer::new().unwrap();
        donor.call_tool("cad_new_project", json!({})).unwrap();
        extrude_offset_box(&mut donor, "Sketch1", -12.0, -2.0);
        extrude_offset_box(&mut donor, "Sketch2", 2.0, 12.0);
        let model = donor.call_tool("cad_project_model", json!({})).unwrap();
        let model_json = model
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&model).unwrap());
        session::write_session(unique, "model.json", &model_json).unwrap();
        session::write_session(
            unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();
        donor
            .call_tool("solid_scene", json!({}))
            .expect("donor scene")
    }

    fn write_three_box_session(unique: &str) -> Value {
        let mut donor = CadServer::new().unwrap();
        donor.call_tool("cad_new_project", json!({})).unwrap();
        extrude_offset_box(&mut donor, "Sketch1", -12.0, -2.0);
        extrude_offset_box(&mut donor, "Sketch2", 2.0, 12.0);
        extrude_offset_box(&mut donor, "Sketch3", 16.0, 26.0);
        let model = donor.call_tool("cad_project_model", json!({})).unwrap();
        let model_json = model
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&model).unwrap());
        session::write_session(unique, "model.json", &model_json).unwrap();
        session::write_session(
            unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                session::now_ms()
            ),
        )
        .unwrap();
        donor
            .call_tool("solid_scene", json!({}))
            .expect("donor scene")
    }

    fn overwrite_published_model_keep_generation(unique: &str, host: &mut CadServer) {
        let exported = host.call_tool("cad_project_model", json!({})).unwrap();
        let model_json = exported
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(&exported).unwrap());
        session::write_session(unique, "model.json", &model_json).unwrap();
    }

    fn apply_inbox_on_separate_host(unique: &str) -> session::ApplyResult {
        session::apply_inbox_op(unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            let result = host.call_tool(name, arguments)?;
            let exported = host.call_tool("cad_project_model", json!({}))?;
            let model_json = exported
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(&exported).unwrap());
            session::publish_applied_snapshot(unique, &model_json)?;
            Ok(result)
        })
        .expect("apply helper should run host on a separate SketchManager")
    }

    /// Joint create/update return a DTO, not a solid update. Old applyInboxNow
    /// fell through to loadDocument() (dirty:false). refreshAfterInboxApply
    /// keeps dirty:true for that path.
    fn assert_joint_dto_result(host_result: &Value, label: &str) {
        let is_solid_update =
            host_result.get("scene").is_some() && host_result.get("document").is_some();
        assert!(
            !is_solid_update,
            "{label}: expected a joint result, not a solid update: {host_result}"
        );
        assert!(
            host_result.get("id").is_some() && host_result.get("kind").is_some(),
            "{label}: expected a joint DTO: {host_result}"
        );
    }

    fn joint_inspect_fields(document: &Value) -> Value {
        let joints = document["joints"].as_array().expect("joints");
        let fields: Vec<Value> = joints
            .iter()
            .map(|joint| {
                json!({
                    "id": joint["id"],
                    "name": joint["name"],
                    "kind": joint["kind"],
                    "limits": joint["limits"],
                    "connector_a": {
                        "body_id": joint["connector_a"]["body_id"],
                        "face_id": joint["connector_a"]["face_id"],
                        "kind": joint["connector_a"]["kind"],
                    },
                    "connector_b": {
                        "body_id": joint["connector_b"]["body_id"],
                        "face_id": joint["connector_b"]["face_id"],
                        "kind": joint["connector_b"]["kind"],
                    },
                    "connector_a_occurrence_id": joint["advanced"]["connector_a_occurrence_id"],
                    "connector_b_occurrence_id": joint["advanced"]["connector_b_occurrence_id"],
                })
            })
            .collect();
        json!(fields)
    }

    fn assert_joint_visible(document: &Value, joint_id: u64, name: &str) {
        let joints = document["joints"].as_array().expect("joints");
        assert!(
            joints
                .iter()
                .any(|joint| { joint["id"].as_u64() == Some(joint_id) && joint["name"] == name }),
            "assembly_document missing {name} ({joint_id}): {document}"
        );
    }

    #[test]
    fn attach_cad_submit_joint_create_update_visible_and_dirty() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-submit-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().expect("two bodies");
        assert_eq!(bodies.len(), 2, "expected two extruded bodies: {scene}");
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();

        let submitted_create = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "Hinge1",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "limits": {"min": -90.0, "max": 90.0}
                    },
                    "base_generation": 1
                }),
            )
            .expect("cad_submit create joint while attached");
        assert_eq!(submitted_create["submitted"], true);
        assert_eq!(submitted_create["applied"], false);

        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.op.name, "assembly_create_joint");
        assert_eq!(created.host_result["name"], "Hinge1");
        assert_eq!(created.host_result["kind"], "revolute");
        let joint_id = created.host_result["id"].as_u64().expect("joint id");
        assert_joint_dto_result(&created.host_result, "create");

        server.call_tool("cad_refresh", json!({})).unwrap();
        let after_create = server
            .call_tool("assembly_document", json!({}))
            .expect("joints after create apply");
        assert_joint_visible(&after_create, joint_id, "Hinge1");

        let mut joint = created.host_result.clone();
        if let Some(object) = joint.as_object_mut() {
            object.remove("_disclosure");
            object.insert("name".to_string(), json!("Hinge1Renamed"));
            object.insert("limits".to_string(), json!({"min": -45.0, "max": 45.0}));
        }
        let generation = session::read_heartbeat_generation(&unique).unwrap();
        let submitted_update = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": joint },
                    "base_generation": generation
                }),
            )
            .expect("cad_submit update joint while attached");
        assert_eq!(submitted_update["submitted"], true);
        assert_eq!(submitted_update["applied"], false);

        let updated = apply_inbox_on_separate_host(&unique);
        assert_eq!(updated.op.name, "assembly_update_joint");
        assert_eq!(updated.host_result["name"], "Hinge1Renamed");
        assert_eq!(updated.host_result["id"].as_u64(), Some(joint_id));
        assert_joint_dto_result(&updated.host_result, "update");

        server.call_tool("cad_refresh", json!({})).unwrap();
        let after_update = server
            .call_tool("assembly_document", json!({}))
            .expect("joints after update apply");
        assert_joint_visible(&after_update, joint_id, "Hinge1Renamed");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_joint_query_validates_against_advertised_update_schema() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let body_b_id = second["scene"]["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|body| body["id"].as_u64().unwrap())
            .max()
            .expect("second body id");
        let scene = server
            .call_tool("solid_scene", json!({}))
            .expect("solid scene");
        let bodies = scene["bodies"].as_array().expect("bodies");
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"].as_u64() == Some(body_b_id))
            .cloned()
            .expect("body B");

        let created = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeSchema",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(&body_a),
                    "connector_b": planar_connector_from_body(&body_b),
                    "grounded_body_id": body_a["id"]
                }),
            )
            .expect("create joint");
        let joint_id = created["id"].as_u64().expect("joint id");

        let document = server
            .call_tool("assembly_document", json!({}))
            .expect("query joints");
        let mut queried = document["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .cloned()
            .expect("queried joint");
        if let Some(object) = queried.as_object_mut() {
            object.remove("_disclosure");
        }

        assert!(
            queried["connector_a"]["source_surface_frame"].is_null(),
            "planar connectors serialize source_surface_frame as null: {queried}"
        );
        assert!(
            queried["limits"].is_null(),
            "unset primary limits serialize as null: {queried}"
        );
        assert!(
            queried["angle_limits"].is_null() && queried["linear_limits"].is_null(),
            "unset primary angle/linear limits serialize as null: {queried}"
        );
        assert!(
            queried["advanced"]["secondary_angle_limits"].is_null()
                && queried["advanced"]["tertiary_angle_limits"].is_null()
                && queried["advanced"]["secondary_linear_limits"].is_null(),
            "unset advanced limits serialize as null: {queried}"
        );

        server
            .call_tool(
                "cad_set_focus",
                json!({ "focus": "assembly", "explicit": true }),
            )
            .unwrap();
        let listed = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list"
            }),
        );
        let tools = listed
            .iter()
            .find(|message| message.get("id") == Some(&json!(1)))
            .and_then(|message| message.pointer("/result/tools"))
            .and_then(Value::as_array)
            .expect("tools/list result");
        let update_schema = tools
            .iter()
            .find(|tool| tool["name"] == "assembly_update_joint")
            .and_then(|tool| tool.get("inputSchema"))
            .cloned()
            .expect("advertised assembly_update_joint schema");

        let update_args = json!({ "joint": queried });
        if let Err(error) = schema_accepts(&update_schema, &update_args) {
            panic!("queried joint failed advertised assembly_update_joint schema: {error}\n{update_args}");
        }

        let responses = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "assembly_update_joint",
                    "arguments": update_args
                }
            }),
        );
        let result = responses
            .iter()
            .find(|message| message.get("id") == Some(&json!(2)))
            .expect("update response");
        assert_eq!(
            result["result"]["isError"], false,
            "schema-valid queried joint must update: {result}"
        );
        assert_eq!(
            result["result"]["structuredContent"]["id"].as_u64(),
            Some(joint_id)
        );
        assert_eq!(result["result"]["structuredContent"]["name"], "HingeSchema");
    }

    #[test]
    fn assembly_update_joint_full_record_rename_preserves_limits() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let _second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"] != body_a["id"])
            .cloned()
            .expect("body B");
        let created = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeLimits",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(&body_a),
                    "connector_b": planar_connector_from_body(&body_b),
                    "grounded_body_id": body_a["id"],
                    "limits": {"min": -90.0, "max": 90.0}
                }),
            )
            .unwrap();
        let joint_id = created["id"].as_u64().unwrap();
        let document = server.call_tool("assembly_document", json!({})).unwrap();
        let mut queried = document["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .cloned()
            .expect("queried joint");
        if let Some(object) = queried.as_object_mut() {
            object.remove("_disclosure");
            object.insert("name".into(), json!("HingeRenamed"));
        }
        let updated = server
            .call_tool("assembly_update_joint", json!({ "joint": queried }))
            .unwrap();
        assert_eq!(updated["name"], "HingeRenamed");
        assert!((updated["limits"]["min"].as_f64().unwrap() + 90.0).abs() < 1e-9);
        assert!((updated["limits"]["max"].as_f64().unwrap() - 90.0).abs() < 1e-9);
        let inspect = server.call_tool("assembly_document", json!({})).unwrap();
        let found = inspect["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .unwrap();
        assert_eq!(found["name"], "HingeRenamed");
        assert!((found["limits"]["min"].as_f64().unwrap() + 90.0).abs() < 1e-9);
        assert!((found["limits"]["max"].as_f64().unwrap() - 90.0).abs() < 1e-9);
    }

    #[test]
    fn assembly_update_joint_omitted_limits_clear_on_replace_all() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let _second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"] != body_a["id"])
            .cloned()
            .expect("body B");
        let created = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeWipe",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(&body_a),
                    "connector_b": planar_connector_from_body(&body_b),
                    "grounded_body_id": body_a["id"],
                    "limits": {"min": -30.0, "max": 30.0}
                }),
            )
            .unwrap();
        let joint_id = created["id"].as_u64().unwrap();
        let mut stripped = created.clone();
        if let Some(object) = stripped.as_object_mut() {
            object.remove("_disclosure");
            object.remove("limits");
            object.remove("angle_limits");
            object.remove("linear_limits");
        }
        let updated = server
            .call_tool("assembly_update_joint", json!({ "joint": stripped }))
            .expect("replace-all update with omitted limits");
        assert!(
            updated["limits"].is_null(),
            "omitted limits must clear on replace-all, not preserve: {updated}"
        );
        let inspect = server.call_tool("assembly_document", json!({})).unwrap();
        let found = inspect["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .unwrap();
        assert!(
            found["limits"].is_null(),
            "document must show cleared limits: {found}"
        );
    }

    #[test]
    fn attach_cad_submit_two_joint_ops_get_distinct_seqs() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-seq-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let args = json!({
            "name": "HingeA",
            "kind": "revolute",
            "connector_a": planar_connector_from_body(&body_a),
            "connector_b": planar_connector_from_body(&body_b),
            "grounded_body_id": body_a["id"]
        });
        let first = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": args,
                    "base_generation": 1
                }),
            )
            .unwrap();
        let second = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeB",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(first["submitted"], true);
        assert_eq!(second["submitted"], true);
        assert_ne!(
            first["seq"], second["seq"],
            "concurrent joint submits must not share inbox seq: {first} {second}"
        );
        let pending = session::pending_inbox_seqs(&unique).unwrap();
        assert_eq!(
            pending.len(),
            2,
            "both joint ops must stay pending: {pending:?}"
        );
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_joint_null_fields_schema_and_script_baseline() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-adv-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();

        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeNull",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "limits": {"min": -15.0, "max": 15.0}
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(submitted["submitted"], true);

        let created = apply_inbox_on_separate_host(&unique);
        assert_joint_dto_result(&created.host_result, "create");
        server.call_tool("cad_refresh", json!({})).unwrap();
        let after_create = server.call_tool("assembly_document", json!({})).unwrap();
        let joint_id = created.host_result["id"].as_u64().unwrap();
        assert_joint_visible(&after_create, joint_id, "HingeNull");

        let mut queried = after_create["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .cloned()
            .unwrap();
        if let Some(object) = queried.as_object_mut() {
            object.remove("_disclosure");
            for key in ["connector_a", "connector_b"] {
                object[key]
                    .as_object_mut()
                    .unwrap()
                    .insert("source_surface_frame".into(), Value::Null);
            }
        }
        server
            .call_tool(
                "cad_set_focus",
                json!({ "focus": "assembly", "explicit": true }),
            )
            .unwrap();
        let listed = handle_message(
            &mut server,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list"
            }),
        );
        let tools = listed
            .iter()
            .find(|message| message.get("id") == Some(&json!(1)))
            .and_then(|message| message.pointer("/result/tools"))
            .and_then(Value::as_array)
            .expect("tools/list");
        let update_schema = tools
            .iter()
            .find(|tool| tool["name"] == "assembly_update_joint")
            .and_then(|tool| tool.get("inputSchema"))
            .cloned()
            .expect("update schema");
        let update_args = json!({ "joint": queried });
        schema_accepts(&update_schema, &update_args).unwrap_or_else(|error| {
            panic!("queried joint failed tools/list schema: {error}\n{update_args}")
        });

        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": queried },
                    "base_generation": generation
                }),
            )
            .expect("submit update with explicit nulls");
        let updated = apply_inbox_on_separate_host(&unique);
        assert_joint_dto_result(&updated.host_result, "update-nulls");
        assert_eq!(updated.host_result["id"].as_u64(), Some(joint_id));

        server.call_tool("cad_refresh", json!({})).unwrap();
        server.call_tool("cad_detach", json!({})).unwrap();
        let script = server.call_tool("cad_script", json!({})).unwrap();
        let calls = script["calls"].as_array().unwrap();
        assert_eq!(
            calls.first().and_then(|call| call["name"].as_str()),
            Some("cad_load_project_model"),
            "refresh baseline after detach: {script}"
        );
        let baseline = calls[0]["arguments"]["model_json"].as_str().unwrap_or("");
        assert!(
            baseline.contains("HingeNull"),
            "applied joint must appear in cad_script replay baseline model_json"
        );
        assert!(
            calls.iter().all(|call| {
                !matches!(
                    call["name"].as_str(),
                    Some("cad_submit" | "assembly_create_joint" | "cad_attach" | "cad_refresh")
                )
            }),
            "replay is load-model, not inbox/session control: {script}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_malformed_joint_payload_is_dead_lettered() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-dead-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_two_box_session(&unique);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {"name": "Broken"},
                    "base_generation": 1
                }),
            )
            .expect("cad_submit accepts the op; apply validates");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterDeadLetter"},
                    "base_generation": 1
                }),
            )
            .expect("second op queued behind malformed joint");
        let err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("malformed joint must fail apply");
        assert!(
            err.contains("missing") || err.contains("connector") || err.contains("invalid"),
            "expected a deserialize/validate error, got {err}"
        );
        let pending = session::pending_inbox_seqs(&unique).unwrap();
        assert_eq!(
            pending,
            vec![2],
            "failed joint must dead-letter so seq 2 can apply: {pending:?}"
        );
        let failed = std::path::Path::new(&dir)
            .join(&unique)
            .join("inbox/failed/1.json");
        assert!(failed.exists(), "expected inbox/failed/1.json");

        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterDeadLetter");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_joint_create_leaves_no_ghost_in_assembly_document() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-ghost-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_two_box_session(&unique);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let before = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            before["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(false),
            "precondition: no joints: {before}"
        );
        let before_next = before["next_joint_id"].as_u64();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {"name": "GhostHinge"},
                    "base_generation": 1
                }),
            )
            .expect("cad_submit accepts the op; apply validates");
        let err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("malformed joint must fail apply");
        assert!(
            err.contains("missing") || err.contains("connector") || err.contains("invalid"),
            "expected a deserialize/validate error, got {err}"
        );
        assert!(
            session::pending_inbox_seqs(&unique).unwrap().is_empty(),
            "failed create must dead-letter"
        );
        let attached = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            attached["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(false),
            "attached memory must not grow a ghost joint: {attached}"
        );
        assert_eq!(
            attached["next_joint_id"].as_u64(),
            before_next,
            "failed create must not consume a joint id on the attached snapshot"
        );
        server.call_tool("cad_refresh", json!({})).unwrap();
        let refreshed = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            refreshed["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(false),
            "published snapshot must not contain a ghost joint: {refreshed}"
        );
        assert_eq!(
            refreshed["next_joint_id"].as_u64(),
            before_next,
            "failed create must not consume a joint id on disk: {refreshed}"
        );
        assert!(
            refreshed["joints"]
                .as_array()
                .unwrap()
                .iter()
                .all(|joint| joint["name"] != "GhostHinge"),
            "no ghost joint name for failed create: {refreshed}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_refresh_and_cad_load_project_model_see_same_joints() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-parity-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();

        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeParity",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "limits": {"min": -30.0, "max": 30.0}
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let created = apply_inbox_on_separate_host(&unique);
        let joint_id = created.host_result["id"].as_u64().unwrap();
        assert_joint_dto_result(&created.host_result, "create-parity");

        let mut joint = created.host_result.clone();
        if let Some(object) = joint.as_object_mut() {
            object.remove("_disclosure");
            object.insert("name".into(), json!("HingeParityRenamed"));
            object.insert("limits".into(), json!({"min": -12.0, "max": 18.0}));
        }
        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": joint },
                    "base_generation": generation
                }),
            )
            .unwrap();
        let updated = apply_inbox_on_separate_host(&unique);
        assert_eq!(updated.host_result["id"].as_u64(), Some(joint_id));
        assert_joint_dto_result(&updated.host_result, "update-parity");

        let load_while_attached = server
            .call_tool(
                "cad_load_project_model",
                json!({ "model_json": session::require_model_json(&unique).unwrap() }),
            )
            .expect_err("attached cad_load_project_model is not an inspect path");
        assert_session_read_only(&load_while_attached);

        server.call_tool("cad_refresh", json!({})).unwrap();
        let via_refresh = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&via_refresh, joint_id, "HingeParityRenamed");

        let model = session::require_model_json(&unique).unwrap();
        let mut loader = CadServer::new().unwrap();
        loader
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .expect("headless cad_load_project_model of published model");
        let via_load = loader.call_tool("assembly_document", json!({})).unwrap();
        assert_eq!(
            joint_inspect_fields(&via_refresh),
            joint_inspect_fields(&via_load),
            "cad_refresh and cad_load_project_model must see the same joints\nrefresh={via_refresh}\nload={via_load}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn joint_inbox_on_part_document_is_typed_reject_no_ghost() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-part-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_one_box_session(&unique);
        let bodies = scene["bodies"].as_array().expect("one body");
        assert_eq!(bodies.len(), 1, "expected a part with one body: {scene}");
        let body = bodies[0].clone();
        let connector = planar_connector_from_body(&body);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let before = server.call_tool("assembly_document", json!({})).unwrap();
        let before_next = before["next_joint_id"].as_u64();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "PartHinge",
                        "kind": "revolute",
                        "connector_a": connector,
                        "connector_b": connector,
                        "grounded_body_id": body["id"]
                    },
                    "base_generation": 1
                }),
            )
            .expect("schema-valid same-body joint still queues");
        let err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("joint on a part must be a typed host reject");
        assert!(
            err.contains("different occurrences")
                || err.contains("occurrence")
                || err.contains("connector"),
            "expected a typed occurrence/connector reject, got {err}"
        );
        assert!(
            session::pending_inbox_seqs(&unique).unwrap().is_empty(),
            "failed part-joint must dead-letter, not stay pending"
        );
        let failed = std::path::Path::new(&dir)
            .join(&unique)
            .join("inbox/failed/1.json");
        assert!(failed.exists(), "expected inbox/failed/1.json");
        let failed_body = std::fs::read_to_string(&failed).unwrap();
        assert!(
            failed_body.contains("different occurrences")
                || failed_body.contains("occurrence")
                || failed_body.contains("connector"),
            "dead-letter must record the typed reason: {failed_body}"
        );

        let attached = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            attached["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(false),
            "part document must not grow a ghost joint: {attached}"
        );
        assert_eq!(
            attached["next_joint_id"].as_u64(),
            before_next,
            "failed part-joint must not consume a joint id"
        );
        server.call_tool("cad_refresh", json!({})).unwrap();
        let refreshed = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            refreshed["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(false),
            "published part must not contain a ghost joint: {refreshed}"
        );
        assert_eq!(refreshed["next_joint_id"].as_u64(), before_next);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extra_unknown_joint_fields_do_not_change_create_or_update_contract() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-unknown-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let create_args = json!({
            "name": "HingeUnknown",
            "kind": "revolute",
            "connector_a": planar_connector_from_body(&body_a),
            "connector_b": planar_connector_from_body(&body_b),
            "grounded_body_id": body_a["id"],
            "unknown_contract_field": "must-not-stick",
            "limits": {"min": -20.0, "max": 20.0, "unknown_limit_field": true}
        });
        assert!(
            schema_accepts(advertised_create_joint_schema(), &create_args).is_err(),
            "advertised create schema must reject unknown fields"
        );
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": create_args,
                    "base_generation": 1
                }),
            )
            .expect("cad_submit does not re-validate inner additionalProperties");
        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.host_result["name"], "HingeUnknown");
        assert_eq!(created.host_result["kind"], "revolute");
        assert!(
            created.host_result.get("unknown_contract_field").is_none(),
            "extra create field must not stick on the joint DTO: {}",
            created.host_result
        );
        assert!(
            created.host_result["limits"]
                .get("unknown_limit_field")
                .is_none(),
            "extra limit field must not stick: {}",
            created.host_result["limits"]
        );
        assert!((created.host_result["limits"]["min"].as_f64().unwrap() + 20.0).abs() < 1e-9);

        let mut joint = created.host_result.clone();
        if let Some(object) = joint.as_object_mut() {
            object.remove("_disclosure");
            object.insert("unknown_update_field".into(), json!("must-not-stick"));
        }
        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": joint, "unknown_update_wrapper": 1 },
                    "base_generation": generation
                }),
            )
            .expect("submit update with extra fields");
        let updated = apply_inbox_on_separate_host(&unique);
        assert_eq!(updated.host_result["id"], created.host_result["id"]);
        assert_eq!(updated.host_result["name"], "HingeUnknown");
        assert!(
            updated.host_result.get("unknown_update_field").is_none()
                && updated.host_result.get("unknown_update_wrapper").is_none(),
            "extra update fields must not stick: {}",
            updated.host_result
        );

        server.call_tool("cad_refresh", json!({})).unwrap();
        let after = server.call_tool("assembly_document", json!({})).unwrap();
        let found = after["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == created.host_result["id"])
            .expect("joint visible after refresh");
        assert!(found.get("unknown_contract_field").is_none());
        assert!(found.get("unknown_update_field").is_none());

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn advertised_update_joint_schema() -> &'static Value {
        tool_specs()
            .iter()
            .find(|spec| spec.name == "assembly_update_joint")
            .map(|spec| &spec.input_schema)
            .expect("assembly_update_joint ToolSpec")
    }

    fn advertised_create_joint_schema() -> &'static Value {
        tool_specs()
            .iter()
            .find(|spec| spec.name == "assembly_create_joint")
            .map(|spec| &spec.input_schema)
            .expect("assembly_create_joint ToolSpec")
    }

    fn probe_connector_occurrence_ids(unique: &str, body_a: &Value, body_b: &Value) -> (u64, u64) {
        let mut probe = CadServer::new().unwrap();
        let model = session::require_model_json(unique).unwrap();
        probe
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .unwrap();
        let created = probe
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "ProbeHinge",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(body_a),
                    "connector_b": planar_connector_from_body(body_b),
                    "grounded_body_id": body_a["id"]
                }),
            )
            .expect("probe create to discover occurrence ids");
        let occ_a = created["advanced"]["connector_a_occurrence_id"]
            .as_u64()
            .expect("probe occ A");
        let occ_b = created["advanced"]["connector_b_occurrence_id"]
            .as_u64()
            .expect("probe occ B");
        (occ_a, occ_b)
    }

    #[test]
    fn assembly_update_joint_id_name_only_rejected_not_a_patch() {
        let spec = tool_specs()
            .iter()
            .find(|spec| spec.name == "assembly_update_joint")
            .expect("spec");
        assert!(
            spec.description.contains("not a patch"),
            "ToolSpec description must not be readable as patch: {}",
            spec.description
        );
        assert!(
            spec.description.contains("Replace-all") || spec.description.contains("replace-all"),
            "ToolSpec must say replace-all: {}",
            spec.description
        );
        let joint_schema = spec.input_schema["properties"]["joint"].clone();
        let required: Vec<&str> = joint_schema["required"]
            .as_array()
            .expect("joint.required")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for field in ["id", "name", "kind", "connector_a", "connector_b"] {
            assert!(
                required.contains(&field),
                "replace-all schema must require {field}, got {required:?}"
            );
        }
        let desc = joint_schema["description"].as_str().unwrap_or("");
        assert!(
            desc.contains("not a patch"),
            "joint schema description must say not a patch: {desc}"
        );

        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let _second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"] != body_a["id"])
            .cloned()
            .expect("body B");
        let created = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeKeep",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(&body_a),
                    "connector_b": planar_connector_from_body(&body_b),
                    "grounded_body_id": body_a["id"],
                    "limits": {"min": -60.0, "max": 60.0}
                }),
            )
            .unwrap();
        let joint_id = created["id"].as_u64().unwrap();
        let partial = json!({ "joint": { "id": joint_id, "name": "HingePatched" } });
        schema_accepts(advertised_update_joint_schema(), &partial)
            .expect_err("id+name-only must fail advertised schema (would look like a patch)");
        let err = server
            .call_tool("assembly_update_joint", partial)
            .expect_err("host must reject id+name-only; it is not a patch DTO");
        assert!(
            err.contains("missing")
                || err.contains("kind")
                || err.contains("connector")
                || err.contains("invalid"),
            "expected serde/required-field error, got {err}"
        );
        let inspect = server.call_tool("assembly_document", json!({})).unwrap();
        let found = inspect["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .unwrap();
        assert_eq!(
            found["name"], "HingeKeep",
            "failed id+name update must not rename: {found}"
        );
        assert!(
            (found["limits"]["min"].as_f64().unwrap() + 60.0).abs() < 1e-9,
            "failed id+name update must not wipe limits: {found}"
        );
    }

    #[test]
    fn assembly_update_joint_explicit_null_and_omitted_keys_both_clear() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let _second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"] != body_a["id"])
            .cloned()
            .expect("body B");
        let connector_a = planar_connector_from_body(&body_a);
        let connector_b = planar_connector_from_body(&body_b);

        let omitted = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeOmitted",
                    "kind": "revolute",
                    "connector_a": connector_a,
                    "connector_b": connector_b,
                    "grounded_body_id": body_a["id"],
                    "limits": {"min": -25.0, "max": 25.0}
                }),
            )
            .unwrap();
        let omitted_id = omitted["id"].as_u64().unwrap();
        let mut stripped = omitted.clone();
        if let Some(object) = stripped.as_object_mut() {
            object.remove("_disclosure");
            object.remove("limits");
            object.remove("angle_limits");
            object.remove("linear_limits");
        }
        let after_omit = server
            .call_tool("assembly_update_joint", json!({ "joint": stripped }))
            .expect("omitted optional keys");
        assert!(
            after_omit["limits"].is_null(),
            "omitted limits key must clear: {after_omit}"
        );

        let explicit = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeExplicitNull",
                    "kind": "revolute",
                    "connector_a": connector_a,
                    "connector_b": connector_b,
                    "grounded_body_id": body_a["id"],
                    "limits": {"min": -35.0, "max": 35.0}
                }),
            )
            .unwrap();
        let explicit_id = explicit["id"].as_u64().unwrap();
        let mut nulled = explicit.clone();
        if let Some(object) = nulled.as_object_mut() {
            object.remove("_disclosure");
            object.insert("limits".into(), Value::Null);
            object.insert("angle_limits".into(), Value::Null);
            object.insert("linear_limits".into(), Value::Null);
        }
        schema_accepts(
            advertised_update_joint_schema(),
            &json!({ "joint": nulled }),
        )
        .unwrap_or_else(|error| panic!("explicit nulls must be schema-valid: {error}\n{nulled}"));
        let after_null = server
            .call_tool("assembly_update_joint", json!({ "joint": nulled }))
            .expect("explicit JSON nulls");
        assert!(
            after_null["limits"].is_null(),
            "explicit JSON null limits must clear: {after_null}"
        );

        let inspect = server.call_tool("assembly_document", json!({})).unwrap();
        for (joint_id, label) in [(omitted_id, "omitted"), (explicit_id, "explicit-null")] {
            let found = inspect["joints"]
                .as_array()
                .unwrap()
                .iter()
                .find(|joint| joint["id"].as_u64() == Some(joint_id))
                .unwrap();
            assert!(
                found["limits"].is_null(),
                "{label} must persist as cleared in document: {found}"
            );
        }
    }

    #[test]
    fn attach_cad_submit_create_then_update_before_refresh() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-norefresh-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();

        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeFast",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "limits": {"min": -10.0, "max": 10.0}
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let created = apply_inbox_on_separate_host(&unique);
        assert_joint_dto_result(&created.host_result, "create-before-refresh");
        let joint_id = created.host_result["id"].as_u64().unwrap();

        let stale = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            stale["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "attached snapshot must stay stale until refresh: {stale}"
        );

        let mut joint = created.host_result.clone();
        if let Some(object) = joint.as_object_mut() {
            object.remove("_disclosure");
            object.insert("name".into(), json!("HingeFastRenamed"));
        }
        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": joint },
                    "base_generation": generation
                }),
            )
            .expect("submit update before cad_refresh");
        let updated = apply_inbox_on_separate_host(&unique);
        assert_eq!(updated.host_result["id"].as_u64(), Some(joint_id));
        assert_eq!(updated.host_result["name"], "HingeFastRenamed");
        assert_joint_dto_result(&updated.host_result, "update-before-refresh");

        server.call_tool("cad_refresh", json!({})).unwrap();
        let after = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&after, joint_id, "HingeFastRenamed");
        assert_eq!(after["joints"].as_array().map(Vec::len), Some(1));

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_detach_mid_inbox_apply_does_not_fork() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-detach-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeDetach",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(session::pending_inbox_seqs(&unique).unwrap(), vec![1]);

        server.call_tool("cad_detach", json!({})).unwrap();
        assert!(server.attached_document_id.is_none());
        let submit_err = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {"name": "ShouldFail"},
                    "base_generation": 1
                }),
            )
            .expect_err("cad_submit after detach must fail");
        let parsed = parse_session_error(&submit_err);
        assert_eq!(parsed["code"], "not_attached");

        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.op.name, "assembly_create_joint");
        assert_eq!(created.host_result["name"], "HingeDetach");
        let joint_id = created.host_result["id"].as_u64().unwrap();
        assert_joint_dto_result(&created.host_result, "apply-after-detach");

        let detached = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            detached["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "apply must not mutate the detached manager (fork): {detached}"
        );

        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let live = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&live, joint_id, "HingeDetach");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_detach_reattach_same_then_apply_does_not_fork() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-reattach-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeReattach",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(session::pending_inbox_seqs(&unique).unwrap(), vec![1]);
        server.call_tool("cad_detach", json!({})).unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let before = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            before["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "reattach loads the published snapshot; pending must not apply yet: {before}"
        );

        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.op.name, "assembly_create_joint");
        assert_eq!(created.host_result["name"], "HingeReattach");
        let joint_id = created.host_result["id"].as_u64().unwrap();
        assert_joint_dto_result(&created.host_result, "apply-after-reattach");

        let attached = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            attached["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "apply after reattach must not mutate the attached manager: {attached}"
        );

        server.call_tool("cad_refresh", json!({})).unwrap();
        let live = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&live, joint_id, "HingeReattach");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_detach_reattach_other_then_apply_stays_identity_bound() {
        let _guard = session::env_lock();
        let session_a = session::test_session_uuid();
        let session_b = loop {
            let candidate = session::test_session_uuid();
            if candidate != session_a {
                break candidate;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-rebind-{session_a}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene_a = write_two_box_session(&session_a);
        write_two_box_session(&session_b);
        let bodies = scene_a["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": session_a}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeOnA",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(session::pending_inbox_seqs(&session_a).unwrap(), vec![1]);
        server.call_tool("cad_detach", json!({})).unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": session_b}))
            .unwrap();
        assert!(
            session::pending_inbox_seqs(&session_b).unwrap().is_empty(),
            "B must not inherit A's pending inbox"
        );
        let b_before = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            b_before["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "B snapshot must stay joint-free: {b_before}"
        );

        let created = apply_inbox_on_separate_host(&session_a);
        assert_eq!(created.op.name, "assembly_create_joint");
        assert_eq!(created.host_result["name"], "HingeOnA");
        let joint_id = created.host_result["id"].as_u64().unwrap();

        let b_after = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            b_after["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "A's apply must not mutate the B attach: {b_after}"
        );
        server.call_tool("cad_refresh", json!({})).unwrap();
        let b_refresh = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            b_refresh["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "refresh of B must not pick up A's joint: {b_refresh}"
        );
        assert!(session::pending_inbox_seqs(&session_b).unwrap().is_empty());
        let no_b = session::apply_inbox_op(&session_b, |_name, _args| {
            panic!("B has no pending inbox op")
        })
        .expect_err("B apply must stay empty");
        assert!(
            no_b.contains("no pending inbox op"),
            "expected empty B inbox, got {no_b}"
        );

        server.call_tool("cad_detach", json!({})).unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": session_a}))
            .unwrap();
        let a_live = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&a_live, joint_id, "HingeOnA");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_wrong_component_is_dead_lettered() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-comp-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let (occ_a, occ_b) = probe_connector_occurrence_ids(&unique, &body_a, &body_b);
        assert_ne!(occ_a, occ_b, "expected two auto-promoted occurrences");

        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeMissingOcc",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "advanced": {
                            "connector_a_occurrence_id": 99999,
                            "connector_b_occurrence_id": occ_b
                        }
                    },
                    "base_generation": 1
                }),
            )
            .expect("schema-valid missing occurrence still queues");

        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeWrongOcc",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "advanced": {
                            "connector_a_occurrence_id": occ_b,
                            "connector_b_occurrence_id": occ_a
                        }
                    },
                    "base_generation": 1
                }),
            )
            .expect("schema-valid swapped occurrence still queues");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterWrongComponent"},
                    "base_generation": 1
                }),
            )
            .expect("third op queued behind bad joints");

        let missing = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("missing occurrence must fail apply");
        assert!(
            missing.contains("occurrence")
                && (missing.contains("99999")
                    || missing.contains("does not contain")
                    || missing.contains("does not exist")),
            "expected typed missing-occurrence error, got {missing}"
        );
        let pending = session::pending_inbox_seqs(&unique).unwrap();
        assert_eq!(
            pending,
            vec![2, 3],
            "missing-occurrence joint must dead-letter: {pending:?}"
        );

        let wrong = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("wrong occurrence/component must fail apply");
        assert!(
            wrong.contains("occurrence") && wrong.contains("does not contain"),
            "expected typed wrong-component error, got {wrong}"
        );
        let pending = session::pending_inbox_seqs(&unique).unwrap();
        assert_eq!(
            pending,
            vec![3],
            "wrong-component joint must dead-letter: {pending:?}"
        );
        assert!(
            std::path::Path::new(&dir)
                .join(&unique)
                .join("inbox/failed/1.json")
                .exists()
                && std::path::Path::new(&dir)
                    .join(&unique)
                    .join("inbox/failed/2.json")
                    .exists(),
            "both bad joint heads must land in inbox/failed"
        );

        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterWrongComponent");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_whitespace_only_joint_name_is_typed_reject() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-ws-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let spec = tool_specs()
            .iter()
            .find(|spec| spec.name == "assembly_create_joint")
            .expect("create spec");
        assert_eq!(
            spec.input_schema["properties"]["name"]["minLength"], 1,
            "empty name is schema-invalid; whitespace is not"
        );
        assert!(
            spec.input_schema["properties"]["name"]
                .get("maxLength")
                .is_none()
                && spec.input_schema["properties"]["name"]
                    .get("pattern")
                    .is_none(),
            "do not invent max-length/pattern on joint names: {}",
            spec.input_schema["properties"]["name"]
        );
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "   ",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .expect("schema-valid whitespace name still queues");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterWhitespaceName"},
                    "base_generation": 1
                }),
            )
            .expect("follow-up queues behind whitespace name");

        let err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("whitespace-only joint name must typed-reject");
        assert!(
            err.contains("requires a name") || err.contains("name"),
            "expected typed empty-name reject, got {err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "whitespace-name head must dead-letter so seq 2 can apply"
        );
        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterWhitespaceName");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_delete_joint_is_available_through_shared_inbox_mapping() {
        assert!(tool_specs()
            .iter()
            .any(|spec| spec.name == "assembly_delete_joint"));
        assert!(limo_cad_mcp_mutate::lookup_mutate("assembly_delete_joint").is_some());
        assert_eq!(
            tags_for_tool("assembly_delete_joint").0,
            FocusPack::Assembly
        );
    }
    #[test]
    fn attach_cad_submit_body_delete_of_jointed_feature_removes_joint() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-body-del-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let feature_a = body_a["feature_id"].as_u64().expect("body A feature");
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeBeforeBodyDelete",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.host_result["name"], "HingeBeforeBodyDelete");
        let joint_id = created.host_result["id"].as_u64().expect("joint id");
        let occ_a = created.host_result["advanced"]["connector_a_occurrence_id"]
            .as_u64()
            .expect("occ A");
        assert_joint_dto_result(&created.host_result, "create-before-body-delete");

        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_delete_feature",
                    "arguments": {"feature_id": feature_a},
                    "base_generation": generation
                }),
            )
            .expect("body-delete feature queues");
        let deleted = apply_inbox_on_separate_host(&unique);
        assert_eq!(deleted.op.name, "solid_delete_feature");
        assert!(
            deleted.host_result.get("scene").is_some()
                && deleted.host_result.get("document").is_some(),
            "body-delete must return a solid update so leftover applySolidUpdate runs: {}",
            deleted.host_result
        );

        let mut probe = CadServer::new().unwrap();
        let model = session::require_model_json(&unique).unwrap();
        probe
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .unwrap();
        let published = probe.call_tool("assembly_document", json!({})).unwrap();
        let joints = published["joints"].as_array().unwrap();
        assert!(
            joints
                .iter()
                .all(|joint| joint["id"].as_u64() != Some(joint_id)),
            "body-delete must remove the joint, not leave a ghost: {published}"
        );
        assert!(
            joints.is_empty(),
            "expected no leftover joint after connector-body delete: {published}"
        );
        let occ_ids: Vec<u64> = published["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|occurrence| occurrence["id"].as_u64())
            .collect();
        let scene_after = probe.call_tool("solid_scene", json!({})).unwrap();
        let live_ids: Vec<u64> = scene_after["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|body| body["id"].as_u64())
            .collect();
        assert!(
            !live_ids.contains(&body_a["id"].as_u64().unwrap()),
            "deleted body must be gone from the scene: {live_ids:?}"
        );
        assert!(
            live_ids.contains(&body_b["id"].as_u64().unwrap()),
            "unrelated body must remain: {live_ids:?}"
        );
        let _ = (occ_a, occ_ids);

        server.call_tool("cad_refresh", json!({})).unwrap();
        let refreshed = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            refreshed["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(false),
            "cad_refresh must not resurrect the deleted joint: {refreshed}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_body_delete_unrelated_feature_keeps_joint() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-unrel-del-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_three_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), 3, "expected three bodies: {scene}");
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let body_c = bodies[2].clone();
        let feature_c = body_c["feature_id"].as_u64().expect("body C feature");
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeKeep",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let created = apply_inbox_on_separate_host(&unique);
        let joint_id = created.host_result["id"].as_u64().expect("joint id");
        let occ_a = created.host_result["advanced"]["connector_a_occurrence_id"].clone();
        let occ_b = created.host_result["advanced"]["connector_b_occurrence_id"].clone();

        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "solid_delete_feature",
                    "arguments": {"feature_id": feature_c},
                    "base_generation": generation
                }),
            )
            .expect("unrelated body-delete queues");
        let deleted = apply_inbox_on_separate_host(&unique);
        assert_eq!(deleted.op.name, "solid_delete_feature");

        let mut probe = CadServer::new().unwrap();
        let model = session::require_model_json(&unique).unwrap();
        probe
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .unwrap();
        let published = probe.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&published, joint_id, "HingeKeep");
        let found = published["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .unwrap();
        assert_eq!(
            found["advanced"]["connector_a_occurrence_id"], occ_a,
            "unrelated body-delete must not retarget occ A: {found}"
        );
        assert_eq!(
            found["advanced"]["connector_b_occurrence_id"], occ_b,
            "unrelated body-delete must not retarget occ B: {found}"
        );
        assert_eq!(found["connector_a"]["body_id"], body_a["id"]);
        assert_eq!(found["connector_b"]["body_id"], body_b["id"]);
        let scene_after = probe.call_tool("solid_scene", json!({})).unwrap();
        let live_ids: Vec<u64> = scene_after["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|body| body["id"].as_u64())
            .collect();
        assert!(
            !live_ids.contains(&body_c["id"].as_u64().unwrap()),
            "unrelated body C must be gone: {live_ids:?}"
        );
        assert_eq!(live_ids.len(), 2, "A and B must remain: {live_ids:?}");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_pending_joint_after_body_delete_is_dead_lettered() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-pend-del-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let body_a_id = body_a["id"].as_u64().expect("body A id");
        let feature_a = body_a["feature_id"].as_u64().expect("body A feature");
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let (occ_a, occ_b) = probe_connector_occurrence_ids(&unique, &body_a, &body_b);
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeAfterGoneBody",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "advanced": {
                            "connector_a_occurrence_id": occ_a,
                            "connector_b_occurrence_id": occ_b
                        }
                    },
                    "base_generation": 1
                }),
            )
            .expect("pending create queues");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterPendingBodyDelete"},
                    "base_generation": 1
                }),
            )
            .expect("follow-up queues behind pending create");

        {
            let mut host = CadServer::new().unwrap();
            let model = session::require_model_json(&unique).unwrap();
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))
                .unwrap();
            host.call_tool("solid_delete_feature", json!({"feature_id": feature_a}))
                .expect("live body-delete of pending create's occ");
            overwrite_published_model_keep_generation(&unique, &mut host);
            assert_eq!(
                session::read_heartbeat_generation(&unique).unwrap(),
                1,
                "live body-delete must keep the pending seq hint"
            );
        }

        let err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("create naming a body-deleted occ must fail apply");
        assert!(
            err.contains("does not exist")
                || err.contains("occurrence")
                || err.contains(&body_a_id.to_string())
                || err.contains(&occ_a.to_string()),
            "expected typed missing-body/occ reject, got {err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "pending create after body-delete must dead-letter so seq 2 can apply"
        );
        assert!(
            std::path::Path::new(&dir)
                .join(&unique)
                .join("inbox/failed/1.json")
                .exists(),
            "pending create must land in inbox/failed"
        );

        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterPendingBodyDelete");

        let mut probe = CadServer::new().unwrap();
        let model = session::require_model_json(&unique).unwrap();
        probe
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .unwrap();
        let published = probe.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            published["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "failed create after body-delete must not ghost a joint: {published}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_update_after_joint_body_delete_is_dead_lettered() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-upd-del-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let feature_a = body_a["feature_id"].as_u64().expect("body A feature");
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeThenDeleted",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let created = apply_inbox_on_separate_host(&unique);
        let joint_id = created.host_result["id"].as_u64().expect("joint id");
        let mut stale = created.host_result.clone();
        if let Some(object) = stale.as_object_mut() {
            object.remove("_disclosure");
            object.insert("name".into(), json!("HingeResurrect"));
        }

        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": stale },
                    "base_generation": generation
                }),
            )
            .expect("schema-valid update still queues");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterUpdateBodyDelete"},
                    "base_generation": generation
                }),
            )
            .expect("follow-up queues behind update");

        {
            let mut host = CadServer::new().unwrap();
            let model = session::require_model_json(&unique).unwrap();
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))
                .unwrap();
            host.call_tool("solid_delete_feature", json!({"feature_id": feature_a}))
                .expect("body-delete of the joint's occurrence");
            overwrite_published_model_keep_generation(&unique, &mut host);
            let gone = host.call_tool("assembly_document", json!({})).unwrap();
            assert!(
                gone["joints"]
                    .as_array()
                    .map(|joints| joints.is_empty())
                    .unwrap_or(false),
                "host cleanup must drop the joint before the pending update: {gone}"
            );
        }

        let err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("update of a body-deleted joint must fail apply");
        assert!(
            (err.contains(&joint_id.to_string()) && err.contains("does not exist"))
                || err.contains("body")
                || err.contains("occurrence"),
            "expected typed gone-joint/body reject, got {err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![3],
            "body-deleted update (seq 2) must dead-letter so seq 3 can apply"
        );

        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterUpdateBodyDelete");

        let mut probe = CadServer::new().unwrap();
        let model = session::require_model_json(&unique).unwrap();
        probe
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .unwrap();
        let published = probe.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            published["joints"]
                .as_array()
                .unwrap()
                .iter()
                .all(|joint| joint["name"] != "HingeResurrect"
                    && joint["id"].as_u64() != Some(joint_id)),
            "failed update must not resurrect the body-deleted joint: {published}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_inverted_limits_is_typed_reject() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-limits-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let spec = tool_specs()
            .iter()
            .find(|spec| spec.name == "assembly_create_joint")
            .expect("create spec");
        let limits_schema = &spec.input_schema["properties"]["limits"];
        let object_limits = limits_schema
            .get("anyOf")
            .or_else(|| limits_schema.get("oneOf"))
            .and_then(Value::as_array)
            .and_then(|alts| {
                alts.iter()
                    .find(|alt| alt.get("type") == Some(&json!("object")))
            })
            .unwrap_or(limits_schema);
        assert_eq!(
            object_limits["properties"]["min"]["type"], "number",
            "do not invent a min range: {}",
            object_limits["properties"]["min"]
        );
        assert_eq!(
            object_limits["properties"]["max"]["type"], "number",
            "do not invent a max range: {}",
            object_limits["properties"]["max"]
        );
        assert!(
            object_limits["properties"]["min"].get("minimum").is_none()
                && object_limits["properties"]["min"].get("maximum").is_none()
                && object_limits["properties"]["max"].get("minimum").is_none()
                && object_limits["properties"]["max"].get("maximum").is_none(),
            "do not invent min/max bounds on limit values: {object_limits}"
        );
        let inverted = json!({"min": 90.0, "max": -90.0});
        schema_accepts(object_limits, &inverted).expect("min>max is schema-valid");

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeInvertedLimits",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "limits": inverted
                    },
                    "base_generation": 1
                }),
            )
            .expect("schema-valid inverted limits still queues");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterInvertedLimits"},
                    "base_generation": 1
                }),
            )
            .expect("follow-up queues behind inverted limits");

        let err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("inverted limits must typed-reject");
        assert!(
            err.contains("invalid motion limits") || err.contains("limits"),
            "expected typed invalid-limits reject, got {err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "inverted-limits head must dead-letter so seq 2 can apply"
        );
        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterInvertedLimits");

        let mut probe = CadServer::new().unwrap();
        let model = session::require_model_json(&unique).unwrap();
        probe
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .unwrap();
        let published = probe.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            published["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "inverted limits must not ghost a joint: {published}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_unicode_joint_name_round_trips() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-unicode-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let name = "ãƒ’ãƒ³ã‚¸Î±-1";
        let spec = tool_specs()
            .iter()
            .find(|spec| spec.name == "assembly_create_joint")
            .expect("create spec");
        assert!(
            spec.input_schema["properties"]["name"]
                .get("pattern")
                .is_none(),
            "do not invent a joint-name pattern: {}",
            spec.input_schema["properties"]["name"]
        );
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": name,
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .expect("unicode name queues");
        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.host_result["name"], name);
        let joint_id = created.host_result["id"].as_u64().expect("joint id");

        server.call_tool("cad_refresh", json!({})).unwrap();
        let refreshed = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&refreshed, joint_id, name);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_json_wrong_tool_name_is_dead_lettered() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-wrong-tool-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let _scene = write_two_box_session(&unique);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let submit_inspect = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_document",
                    "arguments": {},
                    "base_generation": 1
                }),
            )
            .expect_err("inspect tool must not queue via cad_submit");
        assert!(
            submit_inspect.contains("unsupported_inbox_mutate")
                || submit_inspect.contains("unsupported inbox mutate"),
            "cad_submit of inspect tool must be unsupported: {submit_inspect}"
        );
        let submit_delete = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_nonexistent_operation",
                    "arguments": {"id": 1},
                    "base_generation": 1
                }),
            )
            .expect_err("unknown operation must not queue via cad_submit");
        assert!(
            submit_delete.contains("unknown tool") || submit_delete.contains("unsupported"),
            "cad_submit of assembly_nonexistent_operation must stay unknown/unsupported: {submit_delete}"
        );

        session::write_inbox_op(
            &unique,
            &session::InboxOp::unstamped(
                "assembly_nonexistent_operation".to_string(),
                json!({"id": 1}),
                1,
            ),
        )
        .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterWrongTool"},
                    "base_generation": 1
                }),
            )
            .expect("follow-up queues behind raw wrong-tool JSON");
        assert_eq!(session::pending_inbox_seqs(&unique).unwrap(), vec![1, 2]);

        let err = session::apply_inbox_op(&unique, |_name, _args| {
            panic!("host must not run on wrong tool name")
        })
        .expect_err("wrong tool name must fail apply");
        assert!(
            err.contains("unsupported inbox mutate")
                && err.contains("assembly_nonexistent_operation"),
            "expected unsupported-mutate class, got {err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "wrong-tool head must dead-letter so seq 2 can apply"
        );
        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterWrongTool");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_gone_occurrence_after_create_is_dead_lettered() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-gone-occ-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let body_a_id = body_a["id"].as_u64().expect("body A id");
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();

        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeBeforeGone",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .expect("create joint queues");
        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.host_result["name"], "HingeBeforeGone");
        let joint_id = created.host_result["id"].as_u64().expect("joint id");
        let occ_a = created.host_result["advanced"]["connector_a_occurrence_id"]
            .as_u64()
            .expect("occ A");
        let occ_b = created.host_result["advanced"]["connector_b_occurrence_id"]
            .as_u64()
            .expect("occ B");
        assert_ne!(occ_a, occ_b);

        {
            let mut host = CadServer::new().unwrap();
            let model = session::require_model_json(&unique).unwrap();
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))
                .unwrap();
            host.call_tool(
                "assembly_create_component",
                json!({
                    "name": "AbsorbedA",
                    "body_ids": [body_a_id],
                    "absorb_promoted_bodies": true
                }),
            )
            .expect("absorb promoted occurrence A");
            let exported = host.call_tool("cad_project_model", json!({})).unwrap();
            let model_json = exported
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(&exported).unwrap());
            session::publish_applied_snapshot(&unique, &model_json).unwrap();
            let absorbed = host.call_tool("assembly_document", json!({})).unwrap();
            let occ_ids: Vec<u64> = absorbed["component_structure"]["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|occurrence| occurrence["id"].as_u64())
                .collect();
            assert!(
                !occ_ids.contains(&occ_a),
                "absorb must delete occurrence {occ_a}: {occ_ids:?}"
            );
            assert!(
                occ_ids.contains(&occ_b),
                "absorb of A must leave occurrence B: {occ_ids:?}"
            );
            assert_joint_visible(&absorbed, joint_id, "HingeBeforeGone");
        }

        let after_absorb = {
            let mut probe = CadServer::new().unwrap();
            let model = session::require_model_json(&unique).unwrap();
            probe
                .call_tool("cad_load_project_model", json!({ "model_json": model }))
                .unwrap();
            probe.call_tool("assembly_document", json!({})).unwrap()
        };
        let before_next = after_absorb["next_joint_id"].as_u64();
        let live_occ_a = after_absorb["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .and_then(|joint| joint["advanced"]["connector_a_occurrence_id"].as_u64())
            .expect("rewritten occ A");
        assert_ne!(live_occ_a, occ_a, "live joint must not keep the gone occ");

        let mut stale_joint = created.host_result.clone();
        if let Some(object) = stale_joint.as_object_mut() {
            object.remove("_disclosure");
            object.insert("name".into(), json!("HingeGoneOcc"));
        }
        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": stale_joint },
                    "base_generation": generation
                }),
            )
            .expect("schema-valid update naming gone occ still queues");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "GhostAfterGone",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"],
                        "advanced": {
                            "connector_a_occurrence_id": occ_a,
                            "connector_b_occurrence_id": occ_b
                        }
                    },
                    "base_generation": generation
                }),
            )
            .expect("schema-valid create naming gone occ still queues");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterGoneOcc"},
                    "base_generation": generation
                }),
            )
            .expect("follow-up queues behind gone-occ heads");

        let update_err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("update naming gone occurrence must fail apply");
        assert!(
            update_err.contains("occurrence")
                && (update_err.contains(&occ_a.to_string())
                    || update_err.contains("does not contain")
                    || update_err.contains("does not exist")),
            "expected typed gone-occurrence update error, got {update_err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![3, 4],
            "gone-occ update (seq 2) must dead-letter: {:?}",
            session::pending_inbox_seqs(&unique).unwrap()
        );

        let create_err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("create naming gone occurrence must fail apply");
        assert!(
            create_err.contains("occurrence")
                && (create_err.contains(&occ_a.to_string())
                    || create_err.contains("does not contain")
                    || create_err.contains("does not exist")),
            "expected typed gone-occurrence create error, got {create_err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![4],
            "gone-occ create (seq 3) must dead-letter so seq 4 can apply"
        );
        assert!(
            std::path::Path::new(&dir)
                .join(&unique)
                .join("inbox/failed/2.json")
                .exists()
                && std::path::Path::new(&dir)
                    .join(&unique)
                    .join("inbox/failed/3.json")
                    .exists(),
            "both gone-occ heads must land in inbox/failed"
        );

        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterGoneOcc");

        let mut probe = CadServer::new().unwrap();
        let model = session::require_model_json(&unique).unwrap();
        probe
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .unwrap();
        let published = probe.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&published, joint_id, "HingeBeforeGone");
        assert_eq!(
            published["joints"].as_array().map(Vec::len),
            Some(1),
            "failed update/create must not ghost a second joint: {published}"
        );
        assert_eq!(
            published["next_joint_id"].as_u64(),
            before_next,
            "failed create must not consume a joint id: {published}"
        );
        assert!(
            published["joints"]
                .as_array()
                .unwrap()
                .iter()
                .all(|joint| joint["name"] != "HingeGoneOcc" && joint["name"] != "GhostAfterGone"),
            "no ghost names after gone-occ rejects: {published}"
        );
        let published_occ_a =
            published["joints"][0]["advanced"]["connector_a_occurrence_id"].as_u64();
        assert_eq!(
            published_occ_a,
            Some(live_occ_a),
            "failed update must not retarget the absorbed joint"
        );

        let attached = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            attached["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "attached memory stays clean until refresh: {attached}"
        );
        server.call_tool("cad_refresh", json!({})).unwrap();
        let refreshed = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&refreshed, joint_id, "HingeBeforeGone");
        assert_eq!(refreshed["joints"].as_array().map(Vec::len), Some(1));

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_unknown_joint_id_update_is_dead_lettered() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-unknown-id-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeKnown",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let created = apply_inbox_on_separate_host(&unique);
        let joint_id = created.host_result["id"].as_u64().expect("joint id");
        let before_next = {
            let mut probe = CadServer::new().unwrap();
            let model = session::require_model_json(&unique).unwrap();
            probe
                .call_tool("cad_load_project_model", json!({ "model_json": model }))
                .unwrap();
            probe.call_tool("assembly_document", json!({})).unwrap()["next_joint_id"].as_u64()
        };

        let mut unknown = created.host_result.clone();
        if let Some(object) = unknown.as_object_mut() {
            object.remove("_disclosure");
            object.insert("id".into(), json!(99999));
            object.insert("name".into(), json!("HingeUnknown"));
        }
        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": unknown },
                    "base_generation": generation
                }),
            )
            .expect("schema-valid unknown-id update still queues");
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterUnknownJoint"},
                    "base_generation": generation
                }),
            )
            .expect("follow-up queues behind unknown-id update");

        let err = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            host.call_tool(name, arguments)
        })
        .expect_err("unknown joint id must fail apply");
        assert!(
            err.contains("99999") && err.contains("does not exist"),
            "expected typed unknown-joint reject, got {err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![3],
            "unknown-id update (seq 2) must dead-letter so seq 3 can apply"
        );
        assert!(
            std::path::Path::new(&dir)
                .join(&unique)
                .join("inbox/failed/2.json")
                .exists(),
            "unknown-id update must land in inbox/failed"
        );

        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterUnknownJoint");

        let mut probe = CadServer::new().unwrap();
        let model = session::require_model_json(&unique).unwrap();
        probe
            .call_tool("cad_load_project_model", json!({ "model_json": model }))
            .unwrap();
        let published = probe.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&published, joint_id, "HingeKnown");
        assert_eq!(published["joints"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            published["next_joint_id"].as_u64(),
            before_next,
            "unknown-id update must not mint a joint: {published}"
        );
        assert!(
            published["joints"]
                .as_array()
                .unwrap()
                .iter()
                .all(|joint| joint["name"] != "HingeUnknown" && joint["id"].as_u64() != Some(99999)),
            "no ghost unknown joint: {published}"
        );

        server.call_tool("cad_refresh", json!({})).unwrap();
        let refreshed = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&refreshed, joint_id, "HingeKnown");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_create_joint_headless_direct_attached_inbox_only() {
        let mut headless = CadServer::new().unwrap();
        headless.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut headless, "Sketch1", -12.0, -2.0);
        let _second = extrude_offset_box(&mut headless, "Sketch2", 2.0, 12.0);
        let scene = headless.call_tool("solid_scene", json!({})).unwrap();
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"] != body_a["id"])
            .cloned()
            .expect("body B");
        let args = json!({
            "name": "HingeHeadless",
            "kind": "revolute",
            "connector_a": planar_connector_from_body(&body_a),
            "connector_b": planar_connector_from_body(&body_b),
            "grounded_body_id": body_a["id"]
        });
        let created = headless
            .call_tool("assembly_create_joint", args.clone())
            .expect("headless direct assembly_create_joint must work");
        assert_eq!(created["name"], "HingeHeadless");
        let submit_err = headless
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": args,
                    "base_generation": 1
                }),
            )
            .expect_err("cad_submit without attach stays not_attached");
        assert_eq!(parse_session_error(&submit_err)["code"], "not_attached");

        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-modes-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let attached_args = json!({
            "name": "HingeAttached",
            "kind": "revolute",
            "connector_a": planar_connector_from_body(&body_a),
            "connector_b": planar_connector_from_body(&body_b),
            "grounded_body_id": body_a["id"]
        });
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let direct_err = server
            .call_tool("assembly_create_joint", attached_args.clone())
            .expect_err("direct joint create while attached is session_read_only");
        assert_session_read_only(&direct_err);
        let submitted = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": attached_args,
                    "base_generation": 1
                }),
            )
            .expect("attached joint create uses inbox only");
        assert_eq!(submitted["submitted"], true);
        assert_eq!(submitted["applied"], false);
        assert_eq!(submitted["session_mode"], "ui_owned_apply");
        assert_eq!(session::pending_inbox_seqs(&unique).unwrap(), vec![1]);
        let still_empty = server.call_tool("assembly_document", json!({})).unwrap();
        assert!(
            still_empty["joints"]
                .as_array()
                .map(|joints| joints.is_empty())
                .unwrap_or(true),
            "inbox submit must not mutate attached memory: {still_empty}"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn assert_solution_has_joint_occurrences(solution: &Value, occ_a: u64, occ_b: u64) {
        assert!(
            solution.get("body_poses").is_some() && solution.get("occurrence_poses").is_some(),
            "assembly_solution must return pose arrays: {solution}"
        );
        assert!(
            solution.get("solved").and_then(Value::as_bool).is_some(),
            "assembly_solution must report solved: {solution}"
        );
        assert!(
            solution
                .get("diagnostics")
                .and_then(Value::as_array)
                .is_some(),
            "assembly_solution must return diagnostics even if unsolved: {solution}"
        );
        let poses = solution["occurrence_poses"].as_array().unwrap();
        for occ in [occ_a, occ_b] {
            assert!(
                poses
                    .iter()
                    .any(|pose| pose["occurrence_id"].as_u64() == Some(occ)),
                "assembly_solution missing occurrence {occ}: {solution}"
            );
        }
    }

    #[test]
    fn attach_cad_submit_create_joint_then_assembly_solution() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-sol-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let empty = server
            .call_tool("assembly_solution", json!({}))
            .expect("assembly_solution before any joint must not crash");
        assert!(
            empty.get("diagnostics").and_then(Value::as_array).is_some()
                && empty.get("solved").and_then(Value::as_bool).is_some(),
            "empty-graph solution must still be a DTO: {empty}"
        );

        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeSolved",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.op.name, "assembly_create_joint");
        let joint_id = created.host_result["id"].as_u64().unwrap();
        let occ_a = created.host_result["advanced"]["connector_a_occurrence_id"]
            .as_u64()
            .expect("occ A");
        let occ_b = created.host_result["advanced"]["connector_b_occurrence_id"]
            .as_u64()
            .expect("occ B");

        server.call_tool("cad_refresh", json!({})).unwrap();
        assert_joint_visible(
            &server.call_tool("assembly_document", json!({})).unwrap(),
            joint_id,
            "HingeSolved",
        );
        let solution = server
            .call_tool("assembly_solution", json!({}))
            .expect("assembly_solution after inbox create must not crash");
        assert_solution_has_joint_occurrences(&solution, occ_a, occ_b);
        if solution["solved"] == false {
            assert!(
                !solution["diagnostics"].as_array().unwrap().is_empty(),
                "unsolved solution must carry diagnostics: {solution}"
            );
        }

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_two_joints_same_occurrence_pair() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-pair-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let connector_a = planar_connector_from_body(&body_a);
        let connector_b = planar_connector_from_body(&body_b);
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "PairRevolute",
                        "kind": "revolute",
                        "connector_a": connector_a,
                        "connector_b": connector_b,
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let first = apply_inbox_on_separate_host(&unique);
        assert_eq!(first.host_result["name"], "PairRevolute");
        let first_id = first.host_result["id"].as_u64().unwrap();
        server.call_tool("cad_refresh", json!({})).unwrap();

        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "PairSlider",
                        "kind": "slider",
                        "connector_a": connector_a,
                        "connector_b": connector_b,
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": generation
                }),
            )
            .unwrap();
        let second = session::apply_inbox_op(&unique, |name, arguments| {
            let mut host = CadServer::new()?;
            let model = session::require_model_json(&unique)?;
            host.call_tool("cad_load_project_model", json!({ "model_json": model }))?;
            let result = host.call_tool(name, arguments)?;
            let exported = host.call_tool("cad_project_model", json!({}))?;
            let model_json = exported
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(&exported).unwrap());
            session::publish_applied_snapshot(&unique, &model_json)?;
            Ok(result)
        });
        match second {
            Ok(applied) => {
                assert_eq!(applied.op.name, "assembly_create_joint");
                assert_eq!(applied.host_result["name"], "PairSlider");
                let second_id = applied.host_result["id"].as_u64().unwrap();
                assert_ne!(first_id, second_id);
                server.call_tool("cad_refresh", json!({})).unwrap();
                let document = server.call_tool("assembly_document", json!({})).unwrap();
                assert_joint_visible(&document, first_id, "PairRevolute");
                assert_joint_visible(&document, second_id, "PairSlider");
                let solution = server
                    .call_tool("assembly_solution", json!({}))
                    .expect("two-joint solution must not crash");
                assert!(
                    solution.get("solved").and_then(Value::as_bool).is_some(),
                    "two-joint solution must stay a DTO: {solution}"
                );
            }
            Err(error) => {
                assert!(
                    error.contains("joint")
                        || error.contains("occurrence")
                        || error.contains("duplicate")
                        || error.contains("conflict")
                        || error.contains("overconstrain"),
                    "second same-pair joint must be a typed reject, got {error}"
                );
                assert!(
                    session::pending_inbox_seqs(&unique).unwrap().is_empty(),
                    "rejected same-pair joint must dead-letter: {error}"
                );
            }
        }

        let follow = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterPair"},
                    "base_generation": follow
                }),
            )
            .expect("queue must accept a follow-up after the same-pair outcome");
        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterPair");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_create_update_same_base_generation() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-samebase-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let mut probe = CadServer::new().unwrap();
        probe
            .call_tool(
                "cad_load_project_model",
                json!({ "model_json": session::require_model_json(&unique).unwrap() }),
            )
            .unwrap();
        let probed = probe
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeSameBase",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(&body_a),
                    "connector_b": planar_connector_from_body(&body_b),
                    "grounded_body_id": body_a["id"]
                }),
            )
            .unwrap();
        let mut update_joint = probed.clone();
        if let Some(object) = update_joint.as_object_mut() {
            object.remove("_disclosure");
            object.insert("name".into(), json!("HingeSameBaseRenamed"));
        }

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        let created_submit = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_create_joint",
                    "arguments": {
                        "name": "HingeSameBase",
                        "kind": "revolute",
                        "connector_a": planar_connector_from_body(&body_a),
                        "connector_b": planar_connector_from_body(&body_b),
                        "grounded_body_id": body_a["id"]
                    },
                    "base_generation": 1
                }),
            )
            .unwrap();
        let update_submit = server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "assembly_update_joint",
                    "arguments": { "joint": update_joint },
                    "base_generation": 1
                }),
            )
            .unwrap();
        assert_eq!(created_submit["submitted"], true);
        assert_eq!(update_submit["submitted"], true);
        assert_ne!(created_submit["seq"], update_submit["seq"]);
        assert_eq!(session::pending_inbox_seqs(&unique).unwrap(), vec![1, 2]);

        let created = apply_inbox_on_separate_host(&unique);
        assert_eq!(created.op.name, "assembly_create_joint");
        assert_eq!(created.host_result["name"], "HingeSameBase");
        let joint_id = created.host_result["id"].as_u64().unwrap();
        assert_eq!(session::pending_inbox_seqs(&unique).unwrap(), vec![2]);

        let conflict = session::apply_inbox_op(&unique, |_name, _args| {
            panic!("host must not run on generation_conflict")
        })
        .expect_err("same-base update must not apply after create advanced generation");
        let parsed = parse_session_error(&conflict);
        assert_eq!(parsed["code"], "generation_conflict");
        assert!(
            session::pending_inbox_seqs(&unique).unwrap().is_empty(),
            "same-base leftover must dead-letter, not silently drop or wedge"
        );
        let failed = std::path::Path::new(&dir)
            .join(&unique)
            .join("inbox/failed/2.json");
        assert!(failed.exists(), "expected inbox/failed/2.json");
        let failed_body = std::fs::read_to_string(&failed).unwrap();
        assert!(
            failed_body.contains("generation_conflict"),
            "dead-letter must record the reason: {failed_body}"
        );

        server.call_tool("cad_refresh", json!({})).unwrap();
        let after_create = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&after_create, joint_id, "HingeSameBase");
        assert_ne!(
            after_create["joints"][0]["name"], "HingeSameBaseRenamed",
            "dead-lettered update must not rename: {after_create}"
        );

        let generation = session::read_heartbeat_generation(&unique).unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterSameBase"},
                    "base_generation": generation
                }),
            )
            .unwrap();
        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterSameBase");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_malformed_inbox_json_is_dead_lettered() {
        let _guard = session::env_lock();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-sessions-joint-badjson-{unique}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        write_two_box_session(&unique);
        let inbox = std::path::Path::new(&dir).join(&unique).join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("1.json"), "{not-json").unwrap();

        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id": unique}))
            .unwrap();
        server
            .call_tool(
                "cad_submit",
                json!({
                    "name": "cad_set_document_name",
                    "arguments": {"name": "AfterMalformedJson"},
                    "base_generation": 1
                }),
            )
            .expect("valid op queues behind malformed JSON");
        assert_eq!(session::pending_inbox_seqs(&unique).unwrap(), vec![1, 2]);

        let err = session::apply_inbox_op(&unique, |_name, _args| {
            panic!("host must not run on malformed inbox JSON")
        })
        .expect_err("malformed inbox JSON must fail apply");
        assert!(
            err.contains("invalid inbox") || err.contains("expected"),
            "expected a JSON parse error, got {err}"
        );
        assert_eq!(
            session::pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "malformed JSON must dead-letter so seq 2 can apply"
        );
        let failed = std::path::Path::new(&dir)
            .join(&unique)
            .join("inbox/failed/1.json");
        assert!(failed.exists(), "expected inbox/failed/1.json");
        let failed_body = std::fs::read_to_string(&failed).unwrap();
        assert!(
            failed_body.contains("{not-json"),
            "dead-letter must keep the raw bytes: {failed_body}"
        );

        let renamed = apply_inbox_on_separate_host(&unique);
        assert_eq!(renamed.op.name, "cad_set_document_name");
        assert_eq!(renamed.host_result["name"], "AfterMalformedJson");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_update_joint_swapped_connector_occurrence_ids() {
        let mut server = CadServer::new().unwrap();
        server.call_tool("cad_new_project", json!({})).unwrap();
        let first = extrude_offset_box(&mut server, "Sketch1", -12.0, -2.0);
        let _second = extrude_offset_box(&mut server, "Sketch2", 2.0, 12.0);
        let scene = server.call_tool("solid_scene", json!({})).unwrap();
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies
            .iter()
            .find(|body| body["id"] == first["scene"]["bodies"][0]["id"])
            .cloned()
            .expect("body A");
        let body_b = bodies
            .iter()
            .find(|body| body["id"] != body_a["id"])
            .cloned()
            .expect("body B");
        let created = server
            .call_tool(
                "assembly_create_joint",
                json!({
                    "name": "HingeSwap",
                    "kind": "revolute",
                    "connector_a": planar_connector_from_body(&body_a),
                    "connector_b": planar_connector_from_body(&body_b),
                    "grounded_body_id": body_a["id"]
                }),
            )
            .unwrap();
        let joint_id = created["id"].as_u64().unwrap();
        let occ_a = created["advanced"]["connector_a_occurrence_id"]
            .as_u64()
            .unwrap();
        let occ_b = created["advanced"]["connector_b_occurrence_id"]
            .as_u64()
            .unwrap();
        assert_ne!(occ_a, occ_b);

        let mut swapped_ids = created.clone();
        if let Some(object) = swapped_ids.as_object_mut() {
            object.remove("_disclosure");
            object["advanced"]["connector_a_occurrence_id"] = json!(occ_b);
            object["advanced"]["connector_b_occurrence_id"] = json!(occ_a);
        }
        let err = server
            .call_tool("assembly_update_joint", json!({ "joint": swapped_ids }))
            .expect_err("swap-only occurrence ids must be a typed reject");
        assert!(
            err.contains("occurrence")
                && (err.contains("does not contain")
                    || err.contains("binding")
                    || err.contains("connector body")),
            "expected typed occurrence/body mismatch, got {err}"
        );
        let inspect = server.call_tool("assembly_document", json!({})).unwrap();
        let found = inspect["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .unwrap();
        assert_eq!(
            found["advanced"]["connector_a_occurrence_id"].as_u64(),
            Some(occ_a),
            "rejected swap-only ids must not persist: {found}"
        );
        assert_eq!(
            found["advanced"]["connector_b_occurrence_id"].as_u64(),
            Some(occ_b)
        );

        let mut swapped_connectors = created.clone();
        if let Some(object) = swapped_connectors.as_object_mut() {
            object.remove("_disclosure");
            let connector_a = object["connector_a"].clone();
            let connector_b = object["connector_b"].clone();
            object.insert("connector_a".into(), connector_b);
            object.insert("connector_b".into(), connector_a);
            object["advanced"]["connector_a_occurrence_id"] = json!(occ_b);
            object["advanced"]["connector_b_occurrence_id"] = json!(occ_a);
        }
        let updated = server
            .call_tool(
                "assembly_update_joint",
                json!({ "joint": swapped_connectors }),
            )
            .expect("swapping both connectors including occurrence ids is legal");
        assert_eq!(updated["id"].as_u64(), Some(joint_id));
        assert_eq!(
            updated["advanced"]["connector_a_occurrence_id"].as_u64(),
            Some(occ_b)
        );
        assert_eq!(
            updated["advanced"]["connector_b_occurrence_id"].as_u64(),
            Some(occ_a)
        );
        let queried = server.call_tool("assembly_document", json!({})).unwrap();
        let found = queried["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"].as_u64() == Some(joint_id))
            .unwrap();
        assert_eq!(
            found["advanced"]["connector_a_occurrence_id"].as_u64(),
            Some(occ_b),
            "document query must match swapped connectors: {found}"
        );
        assert_eq!(
            found["advanced"]["connector_b_occurrence_id"].as_u64(),
            Some(occ_a)
        );
        assert_eq!(found["connector_a"]["body_id"], body_b["id"]);
        assert_eq!(found["connector_b"]["body_id"], body_a["id"]);
    }

    pub(super) fn schema_accepts(schema: &Value, value: &Value) -> Result<(), String> {
        if let Some(one_of) = schema.get("oneOf").and_then(Value::as_array) {
            let mut errors = Vec::new();
            for (index, alternative) in one_of.iter().enumerate() {
                match schema_accepts(alternative, value) {
                    Ok(()) => return Ok(()),
                    Err(error) => errors.push(format!("[{index}]: {error}")),
                }
            }
            return Err(format!("no oneOf variant matched ({})", errors.join("; ")));
        }

        if let Some(type_value) = schema.get("type") {
            let types: Vec<&str> = match type_value {
                Value::String(name) => vec![name.as_str()],
                Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            let matches_type = match value {
                Value::Null => types.contains(&"null"),
                Value::Object(_) => types.contains(&"object"),
                Value::Array(_) => types.contains(&"array"),
                Value::String(_) => types.contains(&"string"),
                Value::Bool(_) => types.contains(&"boolean"),
                Value::Number(number) if number.is_i64() || number.is_u64() => {
                    types.contains(&"integer") || types.contains(&"number")
                }
                Value::Number(_) => types.contains(&"number"),
            };
            if !matches_type {
                return Err(format!("value {value} is not one of {types:?}"));
            }
        }

        if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
            if !enum_values.contains(value) {
                return Err(format!("value {value} is not in enum {enum_values:?}"));
            }
        }

        if let Value::Object(object) = value {
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                    for key in object.keys() {
                        if !properties.contains_key(key) {
                            return Err(format!("additional property {key}"));
                        }
                    }
                }
                for (key, child) in object {
                    if let Some(child_schema) = properties.get(key) {
                        schema_accepts(child_schema, child)
                            .map_err(|error| format!("{key}: {error}"))?;
                    }
                }
            }
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for field in required {
                    let name = field.as_str().unwrap_or_default();
                    if !object.contains_key(name) {
                        return Err(format!("missing required {name}"));
                    }
                }
            }
        }

        if let Value::Array(items) = value {
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    schema_accepts(item_schema, item)
                        .map_err(|error| format!("items[{index}]: {error}"))?;
                }
            }
        }

        Ok(())
    }
    #[test]
    fn named_print_view_exports_repeats_without_mutating_mechanical_placement() {
        let (mut server, initial) = mcp_box();
        let body = initial["scene"]["bodies"][0]["id"].clone();
        let assembly = server.call_tool("assembly_document", json!({})).unwrap();
        let root = &assembly["component_structure"]["occurrences"][0];
        let repeated = server
            .call_tool(
                "assembly_create_occurrence",
                json!({"component_id":root["component_id"],"name":"Intentional repeat"}),
            )
            .unwrap();
        server.call_tool("assembly_set_occurrence_pose", json!({"occurrence_id":repeated["id"],"local_pose":{"translation":[40.,0.,0.],"rotation":[0.,0.,0.,1.]}})).unwrap();
        let mechanical = server.call_tool("assembly_solution", json!({})).unwrap();
        let before = server.manager.export_project_model().unwrap();
        let view = json!({"name":"Print","camera":{"position":[100.,-100.,100.],"target":[0.,0.,0.],"up":[0.,0.,1.]},
            "visible_body_ids":[body],"print_layout":true,
            "occurrence_offsets":[{"occurrence_id":root["id"],"translation":[10.,20.,-2.],"rotation":[0.,0.,0.,1.]}]});
        server
            .call_tool(
                "set_named_views",
                json!({"views":[view],"expected_model_json":before}),
            )
            .unwrap();
        let saved = server.manager.export_project_model().unwrap();
        assert!(server
            .call_tool(
                "set_named_views",
                json!({"views":[],"expected_model_json":before})
            )
            .is_err());
        assert_eq!(server.manager.export_project_model().unwrap(), saved);
        let layout = server
            .call_tool("named_view_solution", json!({"name":"Print"}))
            .unwrap();
        assert_eq!(layout["instance_body_poses"].as_array().unwrap().len(), 2);
        assert_eq!(
            layout["instance_body_poses"][0]["translation"],
            json!([10., 20., -2.])
        );
        let report = server
            .call_tool(
                "solid_export_preflight",
                json!({"named_view":"Print","expected_model_json":saved}),
            )
            .unwrap();
        assert_eq!(report["layout"]["printable_instances"], 2);
        assert!(report["layout"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "below_bed"));
        let exported = server.call_tool("solid_export_3mf", json!({"named_view":"Print","expected_model_json":saved,"slicer_target":"bambu_studio"})).unwrap();
        let bytes = BASE64
            .decode(exported["bytes_base64"].as_str().unwrap())
            .unwrap();
        let actual = limo_cad_export::test_reader::read_package(&bytes).unwrap();
        assert_eq!(actual.len(), 2);
        assert!(actual[0].vertices.iter().any(|p| (p[2] + 2.).abs() < 1e-5));
        assert_eq!(
            server.call_tool("assembly_solution", json!({})).unwrap(),
            mechanical
        );
        assert_eq!(server.manager.export_project_model().unwrap(), saved);
        assert!(server
            .call_tool(
                "solid_export_3mf",
                json!({"scope":"definition","named_view":"Print"})
            )
            .is_err());
    }

    #[test]
    fn named_views_attached_execute_and_reads_use_the_owning_engine() {
        let _guard = session::env_lock();
        let id = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("limo-cad-named-views-{id}"));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let (update, model) = write_box_session(&id);
        let body_id = update["scene"]["bodies"][0]["id"].clone();
        session::write_session(
            &id,
            "heartbeat.json",
            &json!({
                "updated_ms":session::now_ms(),"generation":1,"interface_version":1
            })
            .to_string(),
        )
        .unwrap();
        let mut client = CadServer::new().unwrap();
        client
            .call_tool("cad_attach", json!({"session_id":id}))
            .unwrap();
        let peer = id.clone();
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let owner = std::thread::spawn(move || {
            let mut host = CadServer::new().unwrap();
            host.call_tool("cad_load_project_model", json!({"model_json":model}))
                .unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            let mut seen = std::collections::HashSet::new();
            loop {
                if stopped.try_recv() != Err(std::sync::mpsc::TryRecvError::Empty) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "MCP workflow timed out"
                );
                if let Some(seq) = session::pending_inbox_seqs(&peer).unwrap().first().copied() {
                    session::apply_inbox_op(&peer, |name, args| {
                        let value = host.call_tool(name, args)?;
                        session::write_session(
                            &peer,
                            &format!("inbox/results/{seq}.json"),
                            &value.to_string(),
                        )?;
                        let model = host
                            .manager
                            .export_project_model()
                            .map_err(|error| error.to_string())?;
                        session::publish_applied_snapshot(&peer, &model)?;
                        let mut heartbeat: Value = serde_json::from_str(
                            &session::read_session_file(&peer, "heartbeat.json")?,
                        )
                        .unwrap();
                        heartbeat["interface_version"] = json!(1);
                        session::write_session(&peer, "heartbeat.json", &heartbeat.to_string())?;
                        Ok(value)
                    })
                    .unwrap();
                }
                if let Ok(entries) =
                    std::fs::read_dir(session::session_dir().join(&peer).join("controls"))
                {
                    for entry in entries.flatten() {
                        if !entry
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".request.json")
                            || !seen.insert(entry.path())
                        {
                            continue;
                        }
                        let request: Value =
                            serde_json::from_str(&std::fs::read_to_string(entry.path()).unwrap())
                                .unwrap();
                        let query = &request["sketch_query"];
                        let method = query["method"].as_str().unwrap();
                        let value = match method {
                            "named_views" => parse_engine_envelope(host::handle(
                                &mut host.manager,
                                method,
                                query["payload"].as_str().unwrap(),
                            ))
                            .unwrap(),
                            "solid_export_3mf" | "solid_export_stl" | "solid_export_preflight" => {
                                host.call_tool(
                                    method,
                                    serde_json::from_str(query["payload"].as_str().unwrap())
                                        .unwrap(),
                                )
                                .unwrap()
                            }
                            other => panic!("unexpected owning-engine query {other}"),
                        };
                        session::write_session(
                            &peer,
                            &format!("controls/{}.result.json", request["id"].as_str().unwrap()),
                            &json!({"status":"applied","value":value}).to_string(),
                        )
                        .unwrap();
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            host.manager.named_views()
        });
        let view = json!({"name":"review","camera":{"position":[40.0,40.0,40.0],"target":[0.0,0.0,0.0],"up":[0.0,0.0,1.0]},
            "visible_body_ids":[body_id],"part_offsets":[{"body_id":body_id,"translation":[0.0,20.0,0.0]}]});
        let execute = |client: &mut CadServer, op: &str, args: Value| {
            client.call_tool("cad_interface",
            json!({"action":"execute","group":"document/appearance","operation":op,"arguments":args})).unwrap()
        };
        execute(&mut client, "upsert_named_view", view.clone());
        execute(&mut client, "recall_named_view", json!({"name":"review"}));
        assert_eq!(
            client.call_tool("named_views", json!({})).unwrap()["active"],
            "review"
        );
        assert_eq!(
            client.manager.named_views().active,
            None,
            "The local file snapshot cannot supply the live active marker"
        );
        let exported = client.call_tool("solid_export_3mf", json!({})).unwrap();
        let meshes = limo_cad_export::test_reader::read_package(
            &BASE64
                .decode(exported["bytes_base64"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(meshes.len(), 1);
        let baseline = client
            .export_mesh("solid_export_3mf", json!({"named_view":""}))
            .unwrap();
        let baseline = limo_cad_export::test_reader::read_package(
            &BASE64
                .decode(baseline["bytes_base64"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        let points = |vertices: &[[f64; 3]], offset: f64| {
            vertices
                .iter()
                .map(|p| {
                    [
                        (p[0] * 1000.0).round() as i64,
                        ((p[1] + offset) * 1000.0).round() as i64,
                        (p[2] * 1000.0).round() as i64,
                    ]
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            points(&meshes[0].vertices, 0.0),
            points(&baseline[0].vertices, 20.0)
        );
        assert_eq!(
            client.call_tool("solid_export_stl", json!({})).unwrap()["format"],
            "stl"
        );
        let report = client
            .call_tool("solid_export_preflight", json!({}))
            .unwrap();
        assert_eq!(report["layout"]["printable_instances"], 1);
        assert_eq!(
            report["layout"]["bed"],
            serde_json::to_value(limo_cad_core::PrintBedDto::default()).unwrap()
        );
        execute(&mut client, "clear_named_view", json!({}));
        execute(
            &mut client,
            "rename_named_view",
            json!({"name":"review","new_name":"detail"}),
        );
        let listed = client.call_tool("named_views", json!({})).unwrap();
        assert_eq!(listed["views"][0]["name"], "detail");
        assert_eq!(listed["views"][0]["camera"], view["camera"]);
        assert!(listed["active"].is_null());
        execute(&mut client, "delete_named_view", json!({"name":"detail"}));
        execute(&mut client, "set_named_views", json!({"views":[view]}));
        assert_eq!(
            client.call_tool("named_views", json!({})).unwrap()["views"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        drop(stop);
        assert_eq!(owner.join().unwrap().views.len(), 1);
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod agent_feedback_tests {
    use super::*;

    fn feedback_script() -> &'static str {
        r#"{"version":1,"name":"feedback","steps":[
            {"id":"stock","call":{"group":"solid/primitives","operation":"solid_box","arguments":{"size":[100,50,10]}}},
            {"id":"scene","call":{"group":"solid/check","operation":"solid_scene","arguments":{}}},
            {"let":{"plate_body":{"$select":{"from":{"$ref":"scene"},"path":"/bodies","take":"first"}}}},
            {"let":{"top":{"$select":{"from":{"$ref":"plate_body"},"path":"/faces","where":{"/plane/normal/2":1,"/plane/origin/2":10},"take":"one"}}}},
            {"id":"pair","call":{"group":"solid/refine","operation":"solid_hole","arguments":{
                "body_id":{"$ref":"plate_body","pointer":"/id"},"face_id":{"$ref":"top","pointer":"/id"},
                "position":{"$project":{"point":[20,25,10],"basis":{"$ref":"top","pointer":"/plane"}}},
                "positions":[{"position":{"$project":{"point":[20,25,10],"basis":{"$ref":"top","pointer":"/plane"}}}},{"position":{"$project":{"point":[24,25,10],"basis":{"$ref":"top","pointer":"/plane"}}}}],
                "diameter":6,"extent":{"type":"through_all"},"style":"simple","counterbore_diameter":0,"counterbore_depth":0,"countersink_diameter":0,"countersink_angle_deg":90,"flip":false}}},
            {"id":"scene2","call":{"group":"solid/check","operation":"solid_scene","arguments":{}}},
            {"let":{"top2":{"$select":{"from":{"$select":{"from":{"$ref":"scene2"},"path":"/bodies","take":"first"}},"path":"/faces","where":{"/plane/normal/2":1,"/plane/origin/2":10},"take":"one"}}}},
            {"id":"lonely","call":{"group":"solid/refine","operation":"solid_hole","arguments":{
                "body_id":{"$ref":"plate_body","pointer":"/id"},"face_id":{"$ref":"top2","pointer":"/id"},
                "position":{"$project":{"point":[60,25,10],"basis":{"$ref":"top2","pointer":"/plane"}}},
                "positions":[{"position":{"$project":{"point":[80,25,10],"basis":{"$ref":"top2","pointer":"/plane"}}}}],
                "diameter":4,"extent":{"type":"distance","depth":30},"style":"simple","counterbore_diameter":0,"counterbore_depth":0,"countersink_diameter":0,"countersink_angle_deg":90,"flip":false}}},
            {"let":{"unused":{"$ref":"lonely","pointer":"/document"}}}
        ]}"#
    }

    #[test]
    fn script_report_carries_a_feature_summary_and_warnings() {
        let mut server = CadServer::new().unwrap();
        let report = server
            .call_tool(
                "cad_interface",
                json!({"action":"script","source":feedback_script()}),
            )
            .unwrap();
        assert_eq!(report["steps_completed"], 9);
        let summary = &report["summary"];
        assert_eq!(summary["hole_source"], "features");
        assert_eq!(summary["hole_count"], 3);
        assert_eq!(summary["bodies"][0]["size"], json!([100.0, 50.0, 10.0]));
        assert!(
            summary.get("holes").is_none(),
            "compact by default: {summary}"
        );
        let classes = summary["holes_by_class"].as_array().unwrap();
        assert_eq!(classes.len(), 2, "{classes:?}");
        let codes: Vec<&str> = report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["code"].as_str().unwrap())
            .collect();
        assert!(codes.contains(&"holes_overlap"), "{codes:?}");
        assert!(codes.contains(&"hole_position_ignored"), "{codes:?}");
        assert!(codes.contains(&"blind_depth_exceeds_body"), "{codes:?}");
        assert!(codes.contains(&"unused_binding"), "{codes:?}");

        let full = server
            .call_tool("cad_interface", json!({"action":"summary"}))
            .unwrap();
        let holes = full["holes"].as_array().unwrap();
        assert_eq!(holes.len(), 3);
        assert!(
            holes
                .iter()
                .any(|h| h["x"] == 80.0 && h["y"] == 25.0 && h["depth"] == 30.0),
            "{holes:?}"
        );
        assert!(
            holes
                .iter()
                .all(|h| h["z"] == 10.0 && h["normal"][2] == 1.0),
            "{holes:?}"
        );
    }

    #[test]
    fn check_matches_expected_holes_and_reports_missing_and_extra() {
        let mut server = CadServer::new().unwrap();
        server
            .call_tool(
                "cad_interface",
                json!({"action":"script","source":feedback_script()}),
            )
            .unwrap();
        let check = server
            .call_tool(
                "cad_interface",
                json!({"action":"check","tolerance_mm":0.5,"expected":{
                "bbox":[100,50,10],
                "holes":[
                    {"x":20,"y":25,"diameter":6,"through":true},
                    {"x":24.3,"y":25,"diameter":6},
                    {"x":80,"y":25,"diameter":5},
                    {"x":90,"y":40,"diameter":4}
                ]}}),
            )
            .unwrap();
        assert_eq!(check["bbox"]["ok"], true, "{check}");
        let holes = &check["holes"];
        assert_eq!(holes["matched"].as_array().unwrap().len(), 3, "{holes}");
        assert_eq!(holes["missing"].as_array().unwrap().len(), 1, "{holes}");
        assert_eq!(holes["missing"][0]["expected_index"], 3);
        assert!(
            holes["missing"][0]["nearest_built"]["offset_mm"]
                .as_f64()
                .unwrap()
                > 10.0
        );
        assert_eq!(holes["extra"].as_array().unwrap().len(), 0);
        let off = holes["matched"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["expected_index"] == 1)
            .unwrap();
        assert!(
            (off["offset_mm"].as_f64().unwrap() - 0.3).abs() < 0.01,
            "{off}"
        );
        let wrong_diameter = holes["matched"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["expected_index"] == 2)
            .unwrap();
        assert_eq!(wrong_diameter["diameter_ok"], false);
        assert_eq!(check["ok"], false);
        assert!(server
            .call_tool("cad_interface", json!({"action":"check"}))
            .is_err());
    }

    #[test]
    fn failing_selector_names_the_values_present() {
        let mut server = CadServer::new().unwrap();
        let error = server
            .call_tool("cad_interface", json!({"action":"script","source":r#"{"version":1,"name":"bad","steps":[
                {"id":"stock","call":{"group":"solid/primitives","operation":"solid_box","arguments":{"size":[100,50,10]}}},
                {"id":"scene","call":{"group":"solid/check","operation":"solid_scene","arguments":{}}},
                {"let":{"top":{"$select":{"from":{"$select":{"from":{"$ref":"scene"},"path":"/bodies","take":"first"}},"path":"/faces","where":{"/plane/normal/2":1,"/plane/origin/2":11.5},"take":"one"}}}}
            ]}"#}))
            .unwrap_err();
        assert!(
            error.contains("matched no geometry among 6 entries"),
            "{error}"
        );
        assert!(error.contains("/plane/origin/2: [0.0, 10.0]"), "{error}");
    }

    #[test]
    fn solid_box_builds_editable_history_at_an_offset_and_traces_once() {
        let mut server = CadServer::new().unwrap();
        let built = server
            .call_tool(
                "solid_box",
                json!({"origin":[5,7,2],"size":[30,20,4],"name":"Riser"}),
            )
            .unwrap();
        assert_eq!(built["body_ids"].as_array().unwrap().len(), 1, "{built}");
        let body = &built["summary"]["bodies"][0];
        assert_eq!(body["bbox_min"], json!([5.0, 7.0, 2.0]), "{body}");
        assert_eq!(body["bbox_max"], json!([35.0, 27.0, 6.0]), "{body}");
        let traced: Vec<&str> = server
            .tool_trace
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect();
        assert_eq!(traced, vec!["solid_box"], "{traced:?}");
        assert!(server
            .call_tool("solid_box", json!({"size":[0,1,1]}))
            .is_err());
        let cut = server
            .call_tool("solid_box", json!({"origin":[10,10,2],"size":[5,5,4],"operation":"cut","target_body_ids":built["body_ids"].clone()}))
            .unwrap();
        assert_eq!(
            cut["summary"]["bodies"].as_array().unwrap().len(),
            1,
            "{cut}"
        );
        assert!(
            cut["summary"]["bodies"][0]["faces"].as_u64().unwrap() > 6,
            "{cut}"
        );
    }

    #[test]
    fn print_tools_read_a_synthetic_sheet_and_check_the_document_holes() {
        let dir = std::env::temp_dir().join(format!("limo-cad-print-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sheet = dir.join("sheet.png");

        let (mut image, ppm, (ox, oy)) = limo_cad_print::synthetic::sheet();
        limo_cad_print::synthetic::symbol(
            &mut image,
            ox as f64 + 50.0 * ppm,
            oy as f64 - 30.0 * ppm,
            12.0,
        );
        limo_cad_print::synthetic::symbol(
            &mut image,
            ox as f64 + 100.0 * ppm,
            oy as f64 - 30.0 * ppm,
            12.0,
        );
        std::fs::write(&sheet, limo_cad_print::synthetic::png(&image)).unwrap();
        let path = sheet.to_string_lossy().to_string();

        let mut server = CadServer::new().unwrap();
        let calibration = server
            .call_tool(
                "print_calibrate",
                json!({"path":path,"length_mm":200,"width_mm":80}),
            )
            .unwrap();
        assert!(
            (calibration["calibration"]["px_per_mm"].as_f64().unwrap() - 5.0).abs() < 0.05,
            "{calibration}"
        );

        server
            .call_tool("cad_interface", json!({"action":"script","source":r#"{"version":1,"name":"two holes","steps":[
                {"id":"stock","call":{"group":"solid/primitives","operation":"solid_box","arguments":{"size":[200,80,10]}}},
                {"id":"scene","call":{"group":"solid/check","operation":"solid_scene","arguments":{}}},
                {"let":{"plate_body":{"$select":{"from":{"$ref":"scene"},"path":"/bodies","take":"first"}}}},
                {"let":{"top":{"$select":{"from":{"$ref":"plate_body"},"path":"/faces","where":{"/plane/normal/2":1,"/plane/origin/2":10},"take":"one"}}}},
                {"id":"holes","call":{"group":"solid/refine","operation":"solid_hole","arguments":{
                    "body_id":{"$ref":"plate_body","pointer":"/id"},"face_id":{"$ref":"top","pointer":"/id"},
                    "position":{"$project":{"point":[50,30,10],"basis":{"$ref":"top","pointer":"/plane"}}},
                    "positions":[{"position":{"$project":{"point":[50,30,10],"basis":{"$ref":"top","pointer":"/plane"}}}},{"position":{"$project":{"point":[150,30,10],"basis":{"$ref":"top","pointer":"/plane"}}}}],
                    "diameter":5,"extent":{"type":"through_all"},"style":"simple","counterbore_diameter":0,"counterbore_depth":0,"countersink_diameter":0,"countersink_angle_deg":90,"flip":false}}}
            ]}"#}))
            .unwrap();
        let probe = server
            .call_tool(
                "print_probe",
                json!({"path":path,"length_mm":200,"width_mm":80}),
            )
            .unwrap();
        assert_eq!(probe["holes"], 2, "{probe}");
        assert_eq!(probe["nothing_drawn_within_search"], 1, "{probe}");
        let items = probe["items"].as_array().unwrap();
        assert_eq!(items[0]["drawn"], "symbol");
        assert_eq!(items[1]["drawn"], "none");

        let symbols = server
            .call_tool(
                "print_symbols",
                json!({"path":path,"length_mm":200,"width_mm":80}),
            )
            .unwrap();
        assert_eq!(symbols["symbols_found"], 2, "{symbols}");
        assert_eq!(symbols["matched"], 1, "{symbols}");
        assert_eq!(symbols["print_only"].as_array().unwrap().len(), 1);

        let out = dir.join("crop.png");
        let crop = server
            .call_tool("print_crop", json!({"path":path,"length_mm":200,"width_mm":80,"region":"0,0,120,60","out_png":out.to_string_lossy()}))
            .unwrap();
        assert_eq!(crop["holes_drawn"], 2, "{crop}");
        assert!(crop["image"]["png_base64"].as_str().unwrap().len() > 100);
        assert!(std::fs::read(&out).unwrap().starts_with(b"\x89PNG"));
        let result = success_result(crop);
        assert_eq!(result["content"][1]["type"], "image");
        assert_eq!(result["content"][1]["mimeType"], "image/png");
        assert_eq!(
            result["structuredContent"]["image"]["png_base64"],
            "(attached as image content)"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_script_auto_refuses_to_call_a_stale_script_lossless() {
        let mut server = CadServer::new().unwrap();
        let authored = r#"{"version":1,"name":"Authored box","starting_state":"empty","steps":[{"id":"only","note":"authored only"}]}"#;
        server.last_script_source = Some(authored.into());
        server.last_script_mutations = server.modeling_mutations;
        let fresh = server
            .export_script(&json!({"from":"auto","name":"Session"}))
            .unwrap();
        assert_eq!(fresh["fidelity"], "lossless_authored");
        assert_eq!(fresh["stale"], false);
        assert!(fresh["source"].as_str().unwrap().contains('\n'));
        assert!(fresh["source"].as_str().unwrap().contains("authored only"));

        server
            .call_tool("solid_box", json!({"size":[10, 10, 10]}))
            .unwrap();
        let auto = server
            .export_script(&json!({"from":"auto","name":"After box"}))
            .unwrap();
        assert_eq!(auto["fidelity"], "lossy_session_trace");
        assert_eq!(auto["stale"], false);
        let auto_source = auto["source"].as_str().unwrap();
        assert!(auto_source.contains("solid_box"), "{auto_source}");
        assert!(!auto_source.contains("authored only"), "{auto_source}");
        assert!(auto["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("stale")));

        let explicit = server
            .export_script(&json!({"from":"last_script"}))
            .unwrap();
        assert_eq!(explicit["fidelity"], "lossless_authored");
        assert_eq!(explicit["stale"], true);
        assert!(explicit["source"]
            .as_str()
            .unwrap()
            .contains("authored only"));

        server.last_script_source = Some(authored.into());
        server.last_script_mutations = server.modeling_mutations;
        let trace_len = server.tool_trace.len();
        server.modeling_mutations += 1;
        assert_eq!(server.tool_trace.len(), trace_len);
        let live = server
            .export_script(&json!({"from":"last_script"}))
            .unwrap();
        assert_eq!(live["stale"], true);
        let live_auto = server
            .export_script(&json!({"from":"auto","name":"Live"}))
            .unwrap();
        assert_eq!(live_auto["fidelity"], "lossy_session_trace");
    }
}
