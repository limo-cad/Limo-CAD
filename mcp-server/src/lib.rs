use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use nbcad_core::BodyId;
use nbcad_export::MeshExportRequest;
use nbcad_mcp_mutate::{self, PayloadKind as Payload};
use nbcad_occt::OcctKernel;
use nbcad_sketch::{host, SketchManager};
use nbcad_solid::{CommitKernelRequest, RecomputePlanDto, StepExportRequest};
use serde_json::{json, Map, Value};

mod desktop;
mod disclosure;
mod drawing_tools;
mod interface;
mod session;

use disclosure::{
    auto_focus_for_tool, tags_for_tool, AdvertisementState, DisclosureMode, DisclosureState,
    FocusPack,
};

const LATEST_PROTOCOL: &str = "2025-06-18";

/// Inspect the same validated JSONC source accepted by `cad_interface/script`.
pub fn inspect_script(arguments: Value) -> Result<Value, String> {
    let source = interface::script_source(&arguments)?;
    let script = nbcad_script::Script::parse(&source)?;
    let mut result = script.metadata();
    result["source"] = Value::String(source);
    if let Some(path) = arguments.get("path") {
        result["path"] = path.clone();
    }
    Ok(result)
}

/// Embedded desktop entry point. Modeling still goes through the live native
/// inbox and the shared interface, exactly as it does for an external MCP call.
pub fn run_script(
    source: &str,
    session_id: Option<&str>,
    mode: &str,
    speed: f64,
) -> Result<Value, String> {
    let mut arguments = json!({"action":"script","source":source,"mode":mode,"speed":speed});
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
    let metadata = nbcad_script::Script::parse(source)?.metadata();
    let steps = metadata["step_count"].as_u64().unwrap_or(0)
        + metadata["check_count"].as_u64().unwrap_or(0);
    if steps > 80 {
        return Err(
            "Preview supports at most 80 steps and checks; run this script in a new design instead"
                .into(),
        );
    }
    run_script(source, None, "fast", 1.0)
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
    pending_recompute_transaction: Option<u64>,
    /// Forward record of successful mutating `tools/call` entries for `cad_script`.
    tool_trace: Vec<Value>,
    /// Scripts use authoritative live results without rebuilding a second
    /// OCCT model after each mutation. Snapshot reads still refresh on demand.
    script_running: bool,
    live_snapshot_dirty: bool,
}

impl CadServer {
    fn new() -> Result<Self, String> {
        Ok(Self {
            manager: SketchManager::new(),
            kernel: OcctKernel::new().map_err(|error| error.to_string())?,
            disclosure: DisclosureState::new(),
            attached_document_id: None,
            attached_generation: None,
            loaded_snapshot_json: None,
            pending_recompute_transaction: None,
            tool_trace: Vec::new(),
            script_running: false,
            live_snapshot_dirty: false,
        })
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let live_mutation = self.attached_document_id.is_some() && is_modeling_mutate(name);
        let trace_args = arguments.clone();
        let result = self.dispatch_tool(name, arguments);
        if result.is_ok() && records_in_script(name) && !live_mutation {
            self.tool_trace.push(json!({
                "name": name,
                "arguments": trace_args,
            }));
        }
        result
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
                nbcad_mcp_mutate::is_live_engine_query(engine_method)
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

        // The operation is identical with or without a renderer. A live
        // document still owns its engine: hide its inbox protocol from callers.
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

        let payload = nbcad_mcp_mutate::encode_payload(payload_kind, &arguments)?;

        let mut value = if let Some(session_id) =
            self.attached_document_id.as_deref().filter(|_| live_query)
        {
            session::request_engine_query(session_id, engine_method, &payload)?
        } else if execution == Execution::Direct {
            if name == "drawing_projection" {
                let request: nbcad_occt::DrawingProjectionRequest =
                    serde_json::from_value(arguments).map_err(|e| e.to_string())?;
                let scene = self.manager.solid_scene();
                if !scene.errors.is_empty() {
                    return Err("Resolve timeline errors before generating a drawing view.".into());
                }
                let mut projection = self
                    .kernel
                    .drawing_projection(&request)
                    .map_err(|e| e.to_string())?;
                projection.anchors =
                    nbcad_occt::drawing_projection_anchors(&scene, &request, &projection)
                        .map_err(|e| e.to_string())?;
                projection.circles =
                    nbcad_occt::drawing_projection_circles(&scene, &request, &projection)
                        .map_err(|e| e.to_string())?;
                serde_json::to_value(projection).map_err(|e| e.to_string())?
            } else if name == "solid_export_step" {
                let request: StepExportRequest = if arguments.is_null() {
                    StepExportRequest::default()
                } else {
                    serde_json::from_value(arguments)
                        .map_err(|error| format!("invalid STEP export request: {error}"))?
                };
                let bytes = self
                    .kernel
                    .export_step(&request)
                    .map_err(|error| error.to_string())?;
                json!({
                    "format": "step",
                    "encoding": "base64",
                    "bytes_base64": BASE64.encode(bytes),
                })
            } else if name == "solid_export_stl" || name == "solid_export_3mf" {
                self.export_mesh(name, arguments)?
            } else if name == "assembly_interference_check" {
                let request = serde_json::from_value(arguments)
                    .map_err(|e| format!("interference request: {e}"))?;
                let solution = self.manager.assembly_solution();
                if !solution.solved {
                    return Err("Cannot inspect interference in an unsolved assembly".into());
                }
                serde_json::to_value(nbcad_occt::exact_interference_report(
                    &self.kernel,
                    &self.manager.solid_scene(),
                    &solution.instance_body_poses,
                    &request,
                )?)
                .map_err(|e| e.to_string())?
            } else if name == "solid_tessellate" {
                self.tessellate_tool(arguments)?
            } else if name == "solid_export_preflight" {
                self.export_preflight_tool()?
            } else if name == "demo_export_pip_3mf" {
                self.demo_pip_3mf_tool(arguments)?
            } else if name == "material_catalog" {
                serde_json::from_str(&nbcad_export::catalog_json())
                    .map_err(|error| format!("catalog json: {error}"))?
            } else if name == "body_appearances" {
                serde_json::to_value(self.manager.body_appearances())
                    .map_err(|error| format!("encode appearances: {error}"))?
            } else if name == "set_body_appearance" {
                self.set_body_appearance_tool(arguments)?
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
            let scene = match self.kernel.recompute(&plan) {
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
            let committed = parse_engine_envelope(host::handle(
                &mut self.manager,
                "solid_commit",
                &serde_json::to_string(&commit)
                    .map_err(|error| format!("could not encode kernel result: {error}"))?,
            ))?;
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
            "cad_cancel_recompute" => {
                if let Some(transaction_id) = self.pending_recompute_transaction.take() {
                    self.manager.cancel_solid_recompute(transaction_id);
                    json!({ "cancelled": true, "transaction_id": transaction_id })
                } else {
                    json!({ "cancelled": false, "reason": "no in-flight solid recompute" })
                }
            }
            "cad_list_sessions" => session::sessions_list_json(),
            "cad_interface" => {
                if arguments["action"].is_null() || arguments["action"] == "catalog" {
                    json!({"groups":interface::groups(),"operations":full_tool_catalog()})
                } else if arguments["action"] == "execute" {
                    self.execute_interface(&arguments)?
                } else if arguments["action"] == "script" {
                    self.execute_script(&arguments)?
                } else if arguments["action"] == "launch" {
                    let mut launched = desktop::launch(&arguments)?;
                    if launched["status"] == "ready" {
                        self.attach_read_only_snapshot(
                            &json!({"session_id":launched["session_id"]}),
                        )?;
                        launched["attached"] = json!(true);
                    }
                    launched
                } else {
                    let mut result =
                        session::request_ui(&arguments, self.attached_document_id.as_deref())?;
                    if result["status"] == "applied" {
                        if let Some(active) =
                            result["active_session_id"].as_str().map(str::to_owned)
                        {
                            if self.attached_document_id.as_deref() != Some(active.as_str()) {
                                if self.script_running {
                                    return Err("Active document changed during script playback; no later commands were submitted".into());
                                }
                                self.attach_read_only_snapshot(&json!({"session_id":active}))?;
                            } else if self.script_running {
                                // Captions, controls and camera acknowledgements
                                // do not require a second OCCT reconstruction.
                                self.live_snapshot_dirty = true;
                            } else if self.load_snapshot_model(&active, true)? {
                                self.apply_snapshot_focus(&active);
                            }
                            result["attached_session_id"] = json!(active);
                        }
                    }
                    result
                }
            }
            "cad_attach" => self.attach_read_only_snapshot(&arguments)?,
            "cad_refresh" => self.refresh_read_only_snapshot()?,
            "cad_detach" => {
                let previous = self.attached_document_id.take();
                self.attached_generation = None;
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
            "cad_compare_solids" => compare_solids_summary(&self.manager.solid_scene()),
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
        let script = nbcad_script::Script::parse(&source)?;
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
        if !active_sketch.is_null()
            || !self.manager.document_dto().features.is_empty()
            || !self.manager.solid_scene().bodies.is_empty()
            || !self.manager.drawing_document().sheets.is_empty()
            || self.manager.assembly_document() != nbcad_sketch::AssemblyDocumentDto::default()
        {
            return Err(
                "Script requires a blank document; create a new file before running it".into(),
            );
        }
        let options = nbcad_script::RunOptions {
            presentation,
            validate: arguments
                .get("validate")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        };
        script.validate_options(options)?;
        if self.attached_document_id.is_some() {
            let mut configure = json!({"action":"presentation","command":"configure","mode":mode});
            if let Some(speed) = arguments.get("speed") {
                configure["speed"] = speed.clone();
            }
            // Pin the document before any live control acknowledgement can
            // follow a newly active tab. Its blank precondition was checked above.
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
        let mut result = nbcad_script::run(
            &script,
            |name, arguments| {
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
        if self.live_snapshot_dirty {
            // Preserve the original step failure if rebuilding the read cache
            // also fails. The live document remains the authoritative result.
            if let Err(error) = self.refresh_read_only_snapshot() {
                if result.is_ok() {
                    result = Err(error);
                }
            }
        }
        if self.attached_document_id.is_some() {
            let presentation_result = match &result {
                Ok(_) => json!({"action":"presentation","command":"finish"}),
                Err(_) => {
                    // Keep transport receipts in the MCP error, rather than
                    // covering the design with raw JSON in the caption card.
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
        result
    }

    fn wait_for_script_presentation(&mut self, initial: &Value) -> Result<(), String> {
        let mut result = initial.clone();
        loop {
            let state = &result["presentation"];
            if state["stopped"] == true {
                return Err("Playback stopped".into());
            }
            if state["step_pending"] == true {
                // Let the runner reach the next modeling operation; the
                // native inbox consumes the permit after that operation.
                return Ok(());
            }
            let wait_ms = state["wait_ms"].as_u64().unwrap_or(0);
            if state["paused"] != true && wait_ms == 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(if state["paused"] == true {
                250
            } else {
                wait_ms.min(250).max(1)
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
        let submitted = self.submit_inbox_op(
            &json!({"name":name,"arguments":payload,"base_generation":generation}),
        )?;
        let seq = submitted["seq"]
            .as_u64()
            .ok_or("submission omitted sequence")?;
        let applied = await_playback_receipt(
            || {
                self.await_inbox_apply(&json!({"seq":seq,"timeout_ms":30000,"refresh":!self.script_running,"poll_ms":10}))
            },
            || {
                session::request_ui(
                    &json!({"action":"presentation","command":"status"}),
                    Some(&session_id),
                )
            },
        )?;
        if applied["status"] != "applied" {
            // Preserve the receipt and sequence: an uncertain operation must
            // never be silently retried by the interface.
            return Err(applied.to_string());
        }
        if self.script_running && applied["model_published"] == true {
            self.live_snapshot_dirty = true;
        }
        // A completed-model refresh already seeded a replay baseline containing
        // this edit. Only active-sketch edits still need an individual entry.
        if applied["refreshed"] != true {
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
    /// Marks attached only after a successful model load (Jack §3).
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
        let Some(session_id) = self.attached_document_id.clone() else {
            return Err("no session attached; call cad_attach first".to_string());
        };
        self.load_snapshot_model(&session_id, false)?;
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
        if nbcad_mcp_mutate::lookup_mutate(name).is_none() {
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
                .with_identity(&identity),
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
            "hint": "UI/engine applies inbox via host::handle; call cad_await_apply (or cad_refresh after publish)",
        }))
    }

    /// Wait for a submitted inbox seq's apply receipt + publisher snapshot.
    /// Optional `refresh` (default true) reloads a newly published completed
    /// model. Active-sketch-only snapshots are reported but not misrepresented
    /// as a model refresh.
    fn await_inbox_apply(&mut self, arguments: &Value) -> Result<Value, String> {
        let Some(session_id) = self.attached_document_id.clone() else {
            return Err(session::not_attached_error());
        };
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
        if refresh && status == "applied" && published && model_published {
            self.load_snapshot_model(&session_id, false)?;
            self.apply_snapshot_focus(&session_id);
            if let Some(object) = result.as_object_mut() {
                object.insert("refreshed".to_string(), Value::Bool(true));
                object.insert(
                    "hint".to_string(),
                    Value::String(
                        "UI applied and published; MCP in-memory snapshot refreshed from disk"
                            .to_string(),
                    ),
                );
            }
        }
        Ok(result)
    }

    /// Observe attached vs live publisher generation, heartbeat age/stale,
    /// pending inbox, and last apply receipt. Headless returns a clear
    /// `not_attached` status object (not an error).
    fn session_status(&self) -> Result<Value, String> {
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
        let scene = match self.kernel.recompute(&plan) {
            Ok(scene) => scene,
            Err(error) => {
                self.manager.cancel_solid_recompute(transaction_id);
                return Err(format!(
                    "session '{session_id}' model failed to recompute: {error}"
                ));
            }
        };
        let _ = parse_engine_envelope(host::handle(
            &mut self.manager,
            "solid_commit",
            &serde_json::to_string(&CommitKernelRequest {
                transaction_id,
                scene,
            })
            .map_err(|e| e.to_string())?,
        ))?;
        // Attach/refresh replace manager from disk; seed a portable script baseline so
        // cad_script can replay on a fresh CadServer without session UUIDs or files.
        // Refresh re-seeds/replaces the baseline the same way (drops post-attach mutates).
        self.seed_script_baseline_from_model(&model_json);
        self.loaded_snapshot_json = Some(model_json);
        self.attached_generation = publication_generation;
        self.live_snapshot_dirty = false;
        Ok(true)
    }

    /// Clear `tool_trace` and seed `cad_load_project_model` with the loaded model JSON.
    fn seed_script_baseline_from_model(&mut self, model_json: &str) {
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
        if !self.manager.solid_scene().errors.is_empty() {
            return Err("Resolve timeline errors before exporting mesh files.".to_string());
        }
        let request: MeshExportRequest = if arguments.is_null() {
            MeshExportRequest::default()
        } else {
            serde_json::from_value(arguments)
                .map_err(|error| format!("bad mesh export arguments: {error}"))?
        };
        let scene = self.manager.solid_scene();
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
        let solution = self.manager.assembly_solution();
        if !solution.solved {
            return Err("Resolve assembly errors before mesh export.".into());
        }
        let instances: Vec<_> = solution
            .instance_body_poses
            .iter()
            .map(|p| nbcad_export::MeshInstance {
                body_id: p.body_id,
                occurrence_id: p.occurrence_id.0,
                translation: p.translation,
                rotation: p.rotation,
                visible: p.visible,
            })
            .collect();
        let meshes =
            nbcad_export::place_mesh_instances(&meshes, &instances).map_err(|e| e.to_string())?;
        let bytes = if name == "solid_export_stl" {
            nbcad_export::write_stl(&meshes).map_err(|error| error.to_string())?
        } else {
            nbcad_export::ExportFacade::export_3mf(&meshes, &appearances, &request)
                .map_err(|error| error.to_string())?
        };
        Ok(json!({
            "format": if name == "solid_export_stl" { "stl" } else { "3mf" },
            "encoding": "base64",
            "slicer_target": request.slicer_target,
            "byte_length": bytes.len(),
            "bytes_base64": BASE64.encode(bytes),
        }))
    }

    fn set_body_appearance_tool(&mut self, arguments: Value) -> Result<Value, String> {
        let appearance = if let Some(preset_id) = arguments
            .get("preset_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            let body_id = arguments
                .get("body_id")
                .and_then(Value::as_u64)
                .ok_or_else(|| "set_body_appearance with preset_id requires body_id".to_string())?;
            let preset = nbcad_export::find_preset(preset_id).ok_or_else(|| {
                format!("unknown material preset_id '{preset_id}' (call material_catalog)")
            })?;
            preset.to_appearance(BodyId(body_id))
        } else {
            serde_json::from_value(arguments).map_err(|error| {
                format!("invalid body appearance (or pass body_id + preset_id): {error}")
            })?
        };
        let appearances = self
            .manager
            .set_body_appearance(appearance)
            .map_err(|error| error.to_string())?;
        Ok(json!({ "body_appearances": appearances }))
    }

    fn tessellate_tool(&mut self, arguments: Value) -> Result<Value, String> {
        if !self.manager.solid_scene().errors.is_empty() {
            return Err("Resolve timeline errors before tessellating.".to_string());
        }
        let request: MeshExportRequest = if arguments.is_null() {
            MeshExportRequest::default()
        } else {
            serde_json::from_value(arguments)
                .map_err(|error| format!("bad tessellate arguments: {error}"))?
        };
        let scene = self.manager.solid_scene();
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
                let mut min = [f32::MAX; 3];
                let mut max = [f32::MIN; 3];
                for p in mesh.positions.chunks_exact(3) {
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

    fn export_preflight_tool(&mut self) -> Result<Value, String> {
        let scene = self.manager.solid_scene();
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
        Ok(json!({
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
        }))
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
                let (m, a) = nbcad_export::print_in_place_clip();
                (m, a, "print_in_place_clip")
            }
            "cam_bolt" | "cam" => {
                let (m, a) = nbcad_export::print_in_place_cam_bolt();
                (m, a, "print_in_place_cam_bolt")
            }
            other => {
                return Err(format!(
                    "unknown demo kind '{other}' (expected cam_bolt or clip)"
                ))
            }
        };
        let bytes = nbcad_export::ExportFacade::export_3mf(&meshes, &appearances, &request)
            .map_err(|error| error.to_string())?;
        Ok(json!({
            "format": "3mf",
            "encoding": "base64",
            "demo": demo,
            "body_count": meshes.len(),
            "clearance_mm": nbcad_export::CLEAR_MM,
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
            .unwrap_or("unknown noBS CAD engine error")
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
    // Only annotate JSON objects so string/array engine payloads (e.g.
    // cad_project_model) keep their historical shapes for goldens/clients.
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
                    "mutates": is_modeling_mutate(tool.name),
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

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
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
    if matches!(name, "drawing_document" | "drawing_projection") {
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
            | "cad_cancel_recompute"
            | "cad_list_sessions"
            | "cad_interface"
            | "cad_attach"
            | "cad_refresh"
            | "cad_detach"
            | "cad_submit"
            | "cad_await_apply"
            | "cad_session_status"
            | "cad_document"
            | "cad_project_model"
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
            | "material_catalog"
            | "body_appearances"
    )
}

fn is_modeling_mutate(name: &str) -> bool {
    nbcad_mcp_mutate::is_inbox_mutate(name)
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

fn tool_specs() -> Vec<ToolSpec> {
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
            "standard": { "type": "string", "enum": ["iso_metric", "unified_inch"] },
            "series": {
                "type": "string",
                "enum": ["metric_coarse", "metric_fine", "unc", "unf"]
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
            "tap_drill_designation": { "type": ["string", "null"] }
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
    // Host UpdateJointRequestDto is replace-all (not ComponentDefinitionPatchDto).
    // Required connectors/kind make id+name-only schema-invalid so a client
    // cannot treat this as a rename patch.
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
            "Rename the active headless noBS CAD document.",
            "document_set_name",
            Payload::Field("name"),
            object_schema(json!({"name": {"type": "string", "minLength": 1}}), &["name"]),
        ),
        ToolSpec::direct(
            "cad_project_model",
            "Export project model",
            "Return the versioned model.json payload used inside a .nbcad project.",
            "project_export_model",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::solid(
            "cad_load_project_model",
            "Load project model",
            "Transactionally load and recompute a noBS CAD model.json payload.",
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
            "Re-enter a finished sketch by name.",
            "edit_sketch",
            Payload::Field("name"),
            object_schema(json!({"name": {"type": "string", "minLength": 1}}), &["name"]),
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
            dto_schema("LockedRectangleRequest: mode, anchor, corner_hint, optional width/height values or text, ctrl_held."),
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
            "Replace-all UpdateJointRequestDto — not a patch. Send the full queried joint (required: id, name, kind, connector_a, connector_b). Id+name-only is schema-invalid. Omitted or JSON-null optional fields (limits, angle_limits, linear_limits, source_surface_frame) both clear those values. Re-canonicalizes connectors against live topology.",
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
            "Set the primary angle (degrees) and linear coordinate (millimetres) without replacing the joint definition. Query assembly_solution for limits and diagnostics.",
            "assembly_set_joint_motion", Payload::Object,
            object_schema(json!({"joint_id":{"type":"integer","minimum":1},"angle_offset_deg":{"type":"number"},"linear_offset_mm":{"type":"number"}}), &["joint_id","angle_offset_deg","linear_offset_mm"]),
        ),
        ToolSpec::direct(
            "solid_export_step",
            "Export STEP",
            "Export selected or all active bodies as AP242 STEP bytes encoded in base64. Prefer solid_export_3mf for slicers.",
            "solid_export_step",
            Payload::Object,
            object_schema(
                json!({
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
                    "linear_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.15},
                    "angular_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.35}
                }),
                &[],
            ),
        ),
        ToolSpec::direct(
            "solid_export_3mf",
            "Export 3MF",
            "Tessellate active bodies into a standard 3MF (mm, basematerials) with optional slicer Metadata (Bambu/Orca/Prusa/Cura). Preferred print handoff vs STEP.",
            "solid_export_3mf",
            Payload::Object,
            object_schema(
                json!({
                    "body_ids": {
                        "type": "array",
                        "items": {"type": "integer", "minimum": 1},
                        "description": "Empty exports every active body."
                    },
                    "linear_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.15},
                    "angular_deflection": {"type": "number", "exclusiveMinimum": 0, "default": 0.35},
                    "include_appearance": {"type": "boolean", "default": true},
                    "slicer_target": {
                        "type": "string",
                        "enum": ["standard", "bambu_studio", "orca_slicer", "prusa_slicer", "cura"],
                        "default": "bambu_studio",
                        "description": "Embed slicer-compatible Metadata plus consortium basematerials."
                    }
                }),
                &[],
            ),
        ),
        ToolSpec::direct(
            "material_catalog",
            "Material catalog",
            "Return built-in filament presets (Generic, Bambu Lab, Prusa, Polymaker, Hatchbox, Overture, Elegoo, Creality, Sunlu, eSun, Anycubic).",
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
            "Assign filament/color to a body. Prefer body_id + preset_id from material_catalog; or pass a full BodyAppearance object.",
            "set_body_appearance",
            Payload::Object,
            object_schema(
                json!({
                    "body_id": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Target body id from solid_scene."
                    },
                    "preset_id": {
                        "type": "string",
                        "description": "Catalog preset id (e.g. bambu.pla.basic.red). When set, other fields are filled from the catalog."
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
                    "diameter_mm": {"type": "number", "exclusiveMinimum": 0}
                }),
                &["body_id"],
            ),
        ),
        ToolSpec::direct(
            "solid_export_preflight",
            "Export preflight",
            "Check timeline errors, active bodies, and appearance coverage before mesh/STEP export.",
            "solid_export_preflight",
            Payload::Empty,
            empty_schema(),
        ),
        ToolSpec::direct(
            "demo_export_pip_3mf",
            "Export PIP demo 3MF",
            "Return a built-in print-in-place demo as base64 3MF (AABB clearance smoke ≥ 0.4 mm). Does not mutate the document. kind=cam_bolt (default, 4-body wedge+dial) or clip (3-body drawer).",
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
                        "default": "bambu_studio"
                    }
                }),
                &[],
            ),
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
                        "enum": ["document", "assembly", "sketch", "solid", "modify", "body_ops", "datums", "history", "inspect", "print"]
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
            "cad_cancel_recompute",
            "Cancel solid recompute",
            "Abort an in-flight solid replay if one is pending in this MCP process.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_list_sessions",
            "List read-only session snapshots",
            "List UUID v4 session directories under NBCAD_SESSION_DIR (skips _* control dirs and non-UUID names). Includes stable window_id / document_id when the UI publisher wrote them, heartbeat age/stale metadata, expiring desktop process leases, and a windows[] projection with authoritative active documents. Use with cad_attach. Snapshot bridge — not a live UI co-link. Stdio headless sessions without UI identity still list.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_interface", "Explore and drive the product interface",
            "Catalog returns shared product groups and typed operations. Execute runs an operation by group and name with identical arguments/results headlessly or live. Script runs one versioned JSONC command file from source or an absolute .nbcad.jsonc path in the current blank document; Rust sequences every operation, stops on failure, and runs final checks by default. Mode fast has no presentation delays; present requires an attached desktop. Presentation provides configure/note/pause/resume/step/stop/status/finish/dismiss/show and speed controls shared with native playback. View supports timed orientation and focus on an active sketch, body, or component. Launch connects a new desktop. Inspect returns rendered controls with fresh opaque target IDs for click/set_value/key. Window close requests guarded application exit; the reply acknowledges the request, not process termination. No selectors or executable script evaluation.",
            object_schema(json!({
                "session_id":{"type":"string"},
                "action":{"type":"string","enum":["catalog","execute","script","presentation","launch","view","inspect","click","double_click","context_menu","set_value","key","window","file","viewport"]},
                "group":{"type":"string"},"operation":{"type":"string"},"arguments":{"type":"object"},
                "executable":{"type":"string"},
                "view":{"type":"string","enum":["current","isometric","top","bottom","front","back","left","right"]},
                "fit":{"type":"boolean"},
                "body_id":{"type":"integer","minimum":0},"component_id":{"type":"integer","minimum":0},
                "duration_ms":{"type":"integer","minimum":0,"maximum":10000},
                "source":{"type":"string","description":"Version 1 JSONC command script; mutually exclusive with path"},
                "validate":{"type":"boolean","default":true},
                "speed":{"type":"number","minimum":0.1,"maximum":16},
                "text":{"type":"string","maxLength":4000},"chapter":{"type":"string","maxLength":200},
                "step_index":{"type":"integer","minimum":0},"step_count":{"type":"integer","minimum":0},
                "gesture":{"type":"string","enum":["move","click","double_click","drag"]},
                "canvas":{"type":"string","enum":["viewport","drawing"]},
                "to":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
                "point":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
                "world":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},
                "shift":{"type":"boolean"},
                "command":{"type":"string","enum":["open","save","rename","configure","note","pause","resume","step","stop","status","finish","dismiss","show"]},
                "path":{"type":"string"},"name":{"type":"string"},
                "overwrite":{"type":"boolean"},"discard_changes":{"type":"boolean"},
                "target":{"type":"string","description":"Fresh inspect control ID, or active_sketch for view"},"value":{"type":"string"},
                "key":{"type":"string","enum":["Enter","Escape","ArrowUp","ArrowDown","ArrowLeft","ArrowRight","Delete","Backspace"]},
                "mode":{"type":"string","enum":["foreground","background","inspect","close","fast","present"]},
                "pace_ms":{"type":"integer","minimum":0,"maximum":2000}
            }), &[]),
        ),
        ToolSpec::control(
            "cad_attach",
            "Attach read-only session snapshot",
            "Load a published snapshot into this MCP process by session_id (UUID), window_id (Tauri label), and/or document_id (native project-session id; UUID still aliases session_id). Requires valid model.json; optional focus.json. Seeds cad_script baseline with cad_load_project_model (loaded model_json). Fails if the target/model is missing, invalid, or ambiguous. writeback must be omitted or false. While attached to a compatible desktop, the same operation calls submit and await live application internally. Older desktops reject before submission. Never writes back to the session dir. Headless goldens skip attach.",
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
                        "description": "Stable Tauri window label published in heartbeat/focus"
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
            "Re-read model.json (and optional focus.json) for the currently attached session; replaces cad_script baseline with cad_load_project_model for the reloaded model. Explicit refresh — MCP does not watch the filesystem.",
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
            "Return this process's successful mutating tool-call sequence as JSON { calls: [{ name, arguments }] }. Portable modeling ops only — skips session-control reads (cad_attach/cad_refresh/cad_detach), inspect/export helpers, failed calls, and cad_script itself. After attach/refresh, the trace baseline is cad_load_project_model with the loaded model_json (refresh replaces that baseline). Does not reverse-engineer STEP feature history.",
            empty_schema(),
        ),
        ToolSpec::control(
            "cad_compare_solids",
            "Compare solid scene metrics",
            "Summarize active bodies from solid_scene: body count plus per-body bbox, vertex_count, and triangle_count from existing mesh fields. Use to check a rebuilt history against an imported reference solid. Does not invent volume.",
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
            "While attached, poll until inbox/applied/<seq>.json or inbox/failed/<seq>.json appears. For applied ops, also wait until an explicit published_generation catches up to the engine. Completed-model publications optionally cad_refresh (refresh default true); active-sketch-only publications return model_published:false, active_sketch_published:true, refreshed:false because model.json intentionally remains the last completed model. timeout_ms 0 is a single status probe. Still snapshot/UI-owned apply — not in-process co-link. Does not write model.json.",
            object_schema(
                json!({
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
    for tool in &mut tools {
        let (pack, spine) = tags_for_tool(tool.name);
        tool.pack = pack;
        tool.spine = spine;
    }
    tools
}

fn records_in_script(name: &str) -> bool {
    if matches!(name, "drawing_document" | "drawing_projection") {
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
            | "material_catalog"
            | "body_appearances"
            | "demo_export_pip_3mf"
    ) {
        return false;
    }
    if name.ends_with("_definitions") || name.contains("_preview_") {
        return false;
    }
    true
}

fn compare_solids_summary(scene: &nbcad_solid::SolidSceneDto) -> Value {
    let bodies: Vec<Value> = scene
        .bodies
        .iter()
        .map(|body| {
            let positions = &body.mesh.positions;
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for chunk in positions.chunks_exact(3) {
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
    let structured = if value.is_object() {
        value.clone()
    } else {
        json!({ "value": value.clone() })
    };
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
        }],
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
                    "capabilities": { "tools": { "listChanged": true } },
                    "serverInfo": {
                        "name": "nbcad",
                        "title": "noBS CAD",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": "This is one persistent headless CAD document. Begin and finish sketches before creating solid features. Use returned stable entity/body/face/edge ids in later calls. Dynamic tool disclosure is enabled; out-of-focus tools remain callable."
                }),
            )]
        }
        "notifications/initialized" | "notifications/cancelled" => Vec::new(),
        "ping" => id.map(|id| response(id, json!({}))).into_iter().collect(),
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
/// client RPC. Used by the stdin+timeout worker (Jack §2) and by unit tests.
fn idle_due_messages(server: &mut CadServer) -> Vec<Value> {
    server.disclosure.tick_soft_expiry();
    let mut outgoing = Vec::new();
    if let Some(notification) = server.disclosure.take_notify_if_due() {
        outgoing.push(notification);
    }
    outgoing
}

fn write_jsonrpc_messages(stdout: &mut impl Write, messages: &[Value]) -> bool {
    for message in messages {
        if serde_json::to_writer(&mut *stdout, message).is_err()
            || writeln!(stdout).is_err()
            || stdout.flush().is_err()
        {
            return false;
        }
    }
    true
}

enum StdinEvent {
    Line(String),
    Eof,
}

/// Run the external MCP stdio transport using the same interface embedded by the desktop.
pub fn run_stdio() {
    let mut server = match CadServer::new() {
        Ok(server) => server,
        Err(error) => {
            eprintln!("noBS CAD MCP startup failed: {error}");
            std::process::exit(1);
        }
    };

    // Jack §2: do not block forever on stdin. A reader thread feeds lines;
    // the main loop wakes on the next disclosure deadline so list_changed /
    // soft-TTL can flush with no later client ping.
    let (tx, rx) = mpsc::channel::<StdinEvent>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(StdinEvent::Line(line)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(StdinEvent::Eof);
    });

    let mut stdout = io::stdout().lock();
    loop {
        let due = idle_due_messages(&mut server);
        if !due.is_empty() {
            if !write_jsonrpc_messages(&mut stdout, &due) {
                break;
            }
            continue;
        }

        let event = match server.disclosure.ms_until_wake() {
            Some(ms) => match rx.recv_timeout(Duration::from_millis(ms.max(1))) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    let due = idle_due_messages(&mut server);
                    if !write_jsonrpc_messages(&mut stdout, &due) {
                        break;
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(event) => event,
                Err(_) => break,
            },
        };

        match event {
            StdinEvent::Eof => break,
            StdinEvent::Line(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                let outgoing = match serde_json::from_str::<Value>(&line) {
                    Ok(message) => handle_message(&mut server, message),
                    Err(error) => vec![error_response(
                        Value::Null,
                        -32700,
                        format!("parse error: {error}"),
                    )],
                };
                if !write_jsonrpc_messages(&mut stdout, &outgoing) {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            // A pause which resumes just before a deadline still gets another
            // complete wait on seq17 rather than a failing zero-time probe.
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let id = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-live-assembly-query-{id}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_box_session(&id);
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":id}))
            .unwrap();
        let cached = server.manager.assembly_document();
        let cached_model = server.loaded_snapshot_json.clone();
        server.live_snapshot_dirty = true;
        // Querying the live document must not touch the stale reconstruction
        // cache: a full reload would fail against this deliberately bad file.
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
        std::env::remove_var("NBCAD_SESSION_DIR");
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
        // These valid small coordinates drift by one ULP in serde_json's
        // default fast float parser. The API and cached reload must keep bits.
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
    fn deferred_live_snapshot_is_refreshed_before_a_query() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let id = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-deferred-script-{id}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
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
        // Headless golden: 4-body print-in-place cam bolt. No cad_attach.
        // Generator asserts pairwise AABB clearance ≥ CLEAR_MM (0.4).
        let (meshes, apps) = nbcad_export::print_in_place_cam_bolt();
        assert_eq!(meshes.len(), 4);
        assert_eq!(apps.len(), 4);
        assert_eq!(nbcad_export::CLEAR_MM, 0.4);

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
        // Headless golden: 3-body captive drawer clip. No cad_attach.
        let (meshes, apps) = nbcad_export::print_in_place_clip();
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
        // Headless golden: same cam bolt, each slicer_target carries distinct Metadata.
        let mut server = CadServer::new().unwrap();
        let cases: &[(&str, Option<&str>, Option<&str>)] = &[
            (
                "bambu_studio",
                Some("Metadata/project_settings.config"),
                Some("Bambu Lab X1 Carbon"),
            ),
            (
                "orca_slicer",
                Some("Metadata/project_settings.config"),
                Some("Orca Generic"),
            ),
            ("prusa_slicer", Some("Metadata/Slic3r_PE.config"), None),
            ("cura", Some("Metadata/cura_materials.json"), None),
            ("standard", None, None),
        ];
        for (target, extra_file, marker) in cases {
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
            }
            if let Some(marker) = marker {
                let mut settings = archive.by_name("Metadata/project_settings.config").unwrap();
                let mut text = String::new();
                std::io::Read::read_to_string(&mut settings, &mut text).unwrap();
                assert!(
                    text.contains(marker),
                    "{target} settings missing {marker}: {text}"
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
        assert!(!names.iter().any(|name| *name == "solid_extrude"));

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
        assert!(names.iter().any(|name| *name == "solid_extrude"));
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
        assert!(!names.iter().any(|name| *name == "sketch_begin"));
        let result = server
            .call_tool(
                "sketch_begin",
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
            )
            .unwrap();
        // Hidden side-call re-promotes the pack (steerability); still no hard jail.
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
                names.iter().any(|name| *name == "cad_get_focus"),
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
        // Clear the focus-change notify so only soft-TTL expiry remains under test.
        DisclosureState::advance_for_test(disclosure::FOCUS_THROTTLE_MS);
        let _ = idle_due_messages(&mut server);

        DisclosureState::advance_for_test(disclosure::SOFT_TTL_MS + 1);
        let after_expiry = idle_due_messages(&mut server);
        // Expiry schedules a throttled notify; may or may not be due in the same tick.
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-attach-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        // Document-name ids are rejected (UUID v4 required).
        assert!(server
            .call_tool("cad_attach", json!({"session_id": "My Document"}))
            .is_err());
        // Missing model must refuse attach (and leave nothing attached).
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_targets_window_id_and_document_id() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-attach-mw-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        // Headless path: UUID document_id alias still works without window identity.
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

        std::env::remove_var("NBCAD_SESSION_DIR");
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-submit-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_box_session_with_identity(
        unique: &str,
        window_id: &str,
        document_id: &str,
        process_id: &str,
    ) -> (Value, String) {
        let (update, model_json) = write_box_session(unique);
        // write_box_session already wrote heartbeat without window identity —
        // overwrite with the published multi-window identity stamp.
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let session_a = session::test_session_uuid();
        let session_b = format!(
            "00000000-0000-4000-8000-{:012x}",
            (session::now_ms().wrapping_add(41)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-clobber-{session_a}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switch_attach_to_b_submit_does_not_land_in_a() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let session_a = session::test_session_uuid();
        let session_b = format!(
            "00000000-0000-4000-8000-{:012x}",
            (session::now_ms().wrapping_add(43)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-switch-{session_a}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_inbox_helper_on_separate_manager_then_refresh_sees_body() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-apply-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_base_generation_is_generation_conflict_and_does_not_apply() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-stale-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-status-tool-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        // UI undo/edit advances live generation while MCP stays on attach base.
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

        // Stale head dead-letters; status shows last failed receipt.
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_session_status_engine_revision_attach_reports_model_fence_stale() {
        // Jack #84: generation=2, published_generation=1, model_generation=1 —
        // cad_attach loads model gen 1; status must not claim stale:false.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-status-engine-rev-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_await_apply_refreshes_after_separate_host_publish() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-await-tool-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
                // Intermediate engine_revision marker (native bump before TS publish).
                session::write_session(
                    &session_for_worker,
                    "heartbeat.json",
                    &format!(
                        r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{session_for_worker}","kind":"engine_revision","session_mode":"ui_owned_apply"}}"#,
                        session::now_ms()
                    ),
                )?;
                std::thread::sleep(std::time::Duration::from_millis(30));
                // The 10 s keepalive can land in this window. It changes kind
                // but must preserve the still-pending publication fence.
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_await_apply_does_not_refresh_active_sketch_only_snapshot() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-await-sketch-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_await_apply_timeout_probe_while_pending() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-await-probe-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
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
        // ZIP local file header
        assert_eq!(&bytes[0..2], b"PK");
    }

    fn parse_3mf_model_mesh(xml: &str) -> nbcad_export::TriangleMesh {
        use nbcad_core::BodyId;
        let mut positions = Vec::new();
        for line in xml.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("<vertex x=\"") {
                let parts: Vec<&str> = rest.split('"').collect();
                if parts.len() >= 6 {
                    let x: f32 = parts[0].parse().unwrap();
                    let y: f32 = parts[2].parse().unwrap();
                    let z: f32 = parts[4].parse().unwrap();
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
        nbcad_export::TriangleMesh {
            body_id: BodyId(1),
            name: "exported".into(),
            positions,
            indices,
        }
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
        nbcad_export::validate_3mf_model_mesh(&mesh).unwrap();
        assert!(mesh.positions.chunks_exact(3).all(|point| point[0] <= 10.));
        let exported = server.call_tool("solid_export_stl", json!({})).unwrap();
        let bytes = BASE64
            .decode(exported["bytes_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 12);
        assert_eq!(server.manager.export_project_model().unwrap(), before);

        // An assembly containing only unused definitions is empty; it must
        // not suddenly fall back to exporting all source geometry.
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
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("3D/3dmodel.model").unwrap(), &mut xml)
            .unwrap();
        assert_eq!(xml.matches("<mesh>").count(), 2);
        let meshes: Vec<_> = xml
            .split("<mesh>")
            .skip(1)
            .map(|part| parse_3mf_model_mesh(part.split("</mesh>").next().unwrap()))
            .collect();
        for mesh in &meshes {
            nbcad_export::validate_3mf_model_mesh(mesh).unwrap();
        }
        let max_x = meshes
            .iter()
            .flat_map(|m| m.positions.chunks_exact(3).map(|p| p[0]))
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(max_x, 110.);
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
    fn occt_box_export_3mf_is_index_welded() {
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
        assert!(tri_count > 0);
        assert!(
            raw_vertex_count >= tri_count * 3 - 2,
            "OCCT soup should emit ~3 positions per triangle (got {raw_vertex_count} verts, {tri_count} tris)"
        );
        assert!(
            nbcad_export::boundary_edge_count(raw) > 0,
            "raw OCCT mesh should have boundary edges before export weld"
        );

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
        assert!(
            vertex_count < tri_count * 3,
            "exported 3MF should be welded ({vertex_count} verts vs {triangle_count} tris)"
        );
        assert!(
            vertex_count <= raw_vertex_count / 2,
            "welded vertex count should be far below raw soup"
        );
        // Planar OCCT box should weld to the 8 corners (not a hand-built fixture).
        assert_eq!(
            vertex_count, 8,
            "OCCT unit-box 3MF should weld to 8 corners (got {vertex_count})"
        );

        let parsed = parse_3mf_model_mesh(&xml);
        assert_eq!(parsed.positions.len() / 3, vertex_count);
        assert_eq!(parsed.triangle_count(), triangle_count);
        assert_eq!(
            nbcad_export::boundary_edge_count(&parsed),
            0,
            "exported 3MF mesh should be manifold (no boundary edges)"
        );
        assert_eq!(
            nbcad_export::invalid_model_edge_count(&parsed),
            0,
            "every exported edge should have two oppositely oriented triangle uses"
        );
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-script-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        // While attached, direct mutates are session_read_only (UI-owned apply).
        // Detach to fork headless so cad_script can record a portable mutate
        // on top of the attach/refresh cad_load_project_model baseline.
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

        std::env::remove_var("NBCAD_SESSION_DIR");
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
                    "center": {"x": 10.0, "y": 20.0},
                    "start": {"x": 10.0, "y": 0.0},
                    "sweep": {"x": 30.0, "y": 20.0},
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

        let update = server
            .call_tool(
                "solid_sweep",
                json!({
                    "profile": {"sketch_name": "Sketch1", "profile_index": 0},
                    "path_sketch_name": "Sketch2",
                    "path_entity_ids": [arcs[0].clone()],
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
        // Headless: new project → box → create component (absorb) → ground → inspect.
        // No cad_attach.
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-lock-submit-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_create_component_accepted_by_cad_submit_classifier() {
        assert!(
            nbcad_mcp_mutate::lookup_mutate("assembly_create_component").is_some(),
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
                    nbcad_mcp_mutate::lookup_mutate(spec.name).is_none(),
                    "read-safe/control {} must not be an inbox mutate",
                    spec.name
                );
                continue;
            }
            let Some(shared) = nbcad_mcp_mutate::lookup_mutate(spec.name) else {
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
                Execution::Direct => nbcad_mcp_mutate::ExecutionKind::Direct,
                Execution::SolidReplay => nbcad_mcp_mutate::ExecutionKind::SolidReplay,
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
            nbcad_mcp_mutate::mutate_specs().len(),
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
        // Joints name occurrence ids, not display names. A later occurrence
        // rename must not retarget or drop the joint.
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
            names.iter().any(|name| *name == "RenamedA")
                && names.iter().any(|name| *name == "RenamedB"),
            "occurrence display names must change: {names:?}"
        );
        assert_ne!(
            joint["name"], "RenamedA",
            "joint must keep its own name, not the occurrence display name"
        );
    }

    #[test]
    fn attached_sketch_reads_use_the_live_engine_without_loading_a_stale_model() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-live-read-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
                        assert!(nbcad_mcp_mutate::is_live_engine_query(method));
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
        for mutate in nbcad_mcp_mutate::mutate_specs() {
            assert!(!nbcad_mcp_mutate::is_live_engine_query(
                mutate.engine_method
            ));
        }
        std::env::remove_var("NBCAD_SESSION_DIR");
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
        // The grouped transport must not add a second copy of the operation,
        // or leak catalog/renderer controls into a portable modeling script.
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-open-replacement-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
                json!({"action":"file","command":"open","path":"replacement.nbcad"}),
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
        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn acknowledged_document_transitions_track_completed_model_fences() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let first = session::test_session_uuid();
        let second = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-status-transition-{first}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        let (_, model) = write_box_session(&first);
        session::write_session(&second, "model.json", &model).unwrap();
        let mut server = CadServer::new().unwrap();
        server
            .call_tool("cad_attach", json!({"session_id":first}))
            .unwrap();
        let baseline = server.tool_trace.clone();

        // Same completed model at a newer revision, then an active-sketch-only
        // publication, then a different document containing identical geometry.
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
        // A rejected refresh cannot stamp its generation on the retained model.
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
        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn script_configuration_cannot_retarget_a_different_active_document() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let target = session::test_session_uuid();
        let other = "fdec1d2c-6dea-4e73-a20e-0ae5a9a90252";
        let dir = std::env::temp_dir().join(format!("nbcad-script-tab-race-{target}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn grouped_interface_rejects_an_old_desktop_before_submitting() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-old-interface-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(dir);
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
            assert_eq!(tool["mutates"], is_modeling_mutate(name));
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
        for spec in nbcad_mcp_mutate::mutate_specs() {
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
    fn assembly_inbox_refresh_clears_joint_motion_preview() {
        // Viewport prefers jointPreviewSolution, then mechanismPreview.solution,
        // then jointMotionPreview.solution, over assemblySolution. Targeted
        // inbox refresh must clear all three stored previews, not only bump
        // their generation counters.
        let source = include_str!("../../src/store/appStore.ts");
        let start = source
            .find("refreshAfterInboxApply: async (opName)")
            .expect("refreshAfterInboxApply");
        let assembly = source[start..]
            .find("if (opName?.startsWith('assembly_'))")
            .expect("assembly inbox branch");
        let branch = &source[start + assembly..];
        let end = branch
            .find("const doc = await engine.getDocument()")
            .expect("end of targeted assembly refresh");
        let targeted = &branch[..end];
        assert!(
            targeted.contains("jointPreviewSolution: null"),
            "assembly inbox refresh must clear jointPreviewSolution"
        );
        assert!(
            targeted.contains("jointMotionPreview: null"),
            "assembly inbox refresh must clear jointMotionPreview so a stale motion pose cannot hide the new assemblySolution"
        );
        assert!(
            targeted.contains("mechanismPreview: null"),
            "assembly inbox refresh must clear mechanismPreview; viewport prefers it over jointMotionPreview and assemblySolution"
        );
        assert!(
            targeted.contains("dirty: true"),
            "assembly inbox refresh must keep dirty:true (never loadDocument)"
        );
    }

    #[test]
    fn apply_inbox_now_never_falls_through_to_load_document() {
        // applyInboxNow: dead-letter / !applied return before any store write.
        // scene+document -> applySolidUpdate (dirty:true). Else
        // refreshAfterInboxApply (dirty:true). The old loadDocument() else
        // branch cleared dirty and left previews in place.
        let source = include_str!("../../src/sessionBridge.ts");
        let start = source
            .find("async function applyInboxNow()")
            .expect("applyInboxNow");
        let end = source[start..]
            .find("async function heartbeatNow()")
            .expect("end of applyInboxNow");
        let body = &source[start..start + end];
        assert!(
            !body.contains("loadDocument("),
            "applyInboxNow must not call loadDocument (dirty:false): {body}"
        );
        let not_applied = body
            .find("if (!result?.applied) return;")
            .expect("early return when not applied");
        let solid = body.find("applySolidUpdate").expect("solid-update path");
        let refresh = body
            .find("refreshAfterInboxApply")
            .expect("dirty refresh path");
        assert!(
            not_applied < solid && not_applied < refresh,
            "already-applied / empty inbox must no-op before any store mutation"
        );
        assert!(
            body.contains("refreshAfterInboxApply(result.name)"),
            "non-solid inbox results (joint DTO, assembly document) must take refreshAfterInboxApply"
        );
        // applyInboxAll does not exist. A drain-all sibling must not reintroduce
        // loadDocument / dirty:false or skip the already-applied early return.
        assert!(
            !source.contains("applyInboxAll")
                && !source.contains("apply_inbox_all")
                && !source.contains("applyAllInbox"),
            "leftover apply-all must not exist beside applyInboxNow: {source}"
        );
        let store = include_str!("../../src/store/appStore.ts");
        let refresh_defs = store
            .matches("refreshAfterInboxApply: async (opName)")
            .count();
        assert_eq!(
            refresh_defs, 1,
            "leftover refreshAfterInboxApply must be the single #63 path, not a leftover copy"
        );
        let refresh_start = store
            .find("refreshAfterInboxApply: async (opName)")
            .expect("refreshAfterInboxApply");
        let refresh_end = store[refresh_start..]
            .find("setDocument:")
            .expect("end of leftover refreshAfterInboxApply");
        let leftover = &store[refresh_start..refresh_start + refresh_end];
        assert!(
            !leftover.contains("loadDocument("),
            "leftover refreshAfterInboxApply must stay on the #63 contract (no loadDocument): {leftover}"
        );
        assert!(
            leftover.contains("dirty: true"),
            "leftover refreshAfterInboxApply must keep dirty:true"
        );
        assert!(
            leftover.contains("jointPreviewSolution: null")
                && leftover.contains("jointMotionPreview: null")
                && leftover.contains("mechanismPreview: null"),
            "leftover assembly refresh must clear all three previews"
        );
        // solid_delete_feature returns scene+document, so applyInboxNow takes
        // applySolidUpdate — not refreshAfterInboxApply. Body-delete cleanup
        // must still drop stale joint selection and the three previews, then
        // re-read assemblyDocument so a leftover joint ghost cannot linger.
        let apply_solid = store
            .find("applySolidUpdate: (update) => {")
            .expect("applySolidUpdate");
        let apply_solid_end = store[apply_solid..]
            .find("applyDatumPlaneUpdate:")
            .expect("end applySolidUpdate");
        let solid_fn = &store[apply_solid..apply_solid + apply_solid_end];
        assert!(
            solid_fn.contains("dirty: true"),
            "applySolidUpdate (body-delete inbox) must keep dirty:true: {solid_fn}"
        );
        assert!(
            solid_fn.contains("jointPreviewSolution: null")
                && solid_fn.contains("jointMotionPreview: null")
                && solid_fn.contains("mechanismPreview: null"),
            "applySolidUpdate must clear all three previews after body-delete: {solid_fn}"
        );
        assert!(
            solid_fn.contains("engine.assemblyDocument()")
                && solid_fn.contains("engine.assemblySolution()")
                && solid_fn.contains("jointStillExists")
                && solid_fn.contains("selectedJointId"),
            "applySolidUpdate must refresh assembly and drop a deleted joint selection: {solid_fn}"
        );
        // Native apply archives + bumps before JS leftover refresh. A throw
        // after that success must still publish so cad_refresh sees the joint.
        let publish = body
            .find("scheduleSessionBridgePublish()")
            .expect("publish after native apply");
        assert!(
            publish > not_applied && publish > solid && publish > refresh,
            "publish must follow the applied:true leftover refresh, not run on empty/dead-letter"
        );
        assert!(
            body.contains("} finally {")
                && body[publish..].contains("scheduleSessionBridgePublish()"),
            "publish must live in a finally so a leftover refresh throw cannot skip it: {body}"
        );
        let dead = body
            .find("if (result?.dead_lettered)")
            .expect("dead-lettered return");
        assert!(
            dead < publish,
            "dead-letter must return before finally publish so the next applyInboxNow tick can take seq 2"
        );
        assert!(
            body.contains("queue unblocked"),
            "dead-letter path must leave the poller free to apply the lowest remaining pending seq"
        );
        // Pass 7 moved publish into finally. Write must still carry the
        // reserved session/project identity — never active_mut() / later attach.
        let publish_now = source
            .find("async function publishNow()")
            .expect("publishNow");
        let publish_now_end = source[publish_now..]
            .find("async function applyInboxNow()")
            .expect("end publishNow");
        let publish_body = &source[publish_now..publish_now + publish_now_end];
        assert!(
            publish_body.contains("session_id: reservation.session_id")
                && publish_body.contains("project_session_id: reservation.project_session_id"),
            "finally publish must write reserved session_id + project_session_id: {publish_body}"
        );
        assert!(
            !publish_body.contains("active_mut"),
            "JS publish must not target active_mut(): {publish_body}"
        );
        let native = include_str!("../../src-tauri/src/session_bridge.rs");
        let write_start = native
            .find("fn write_for_window(")
            .expect("write_for_window");
        let write_end = native[write_start..]
            .find("fn heartbeat_for_window(")
            .expect("end write_for_window");
        let write_fn = &native[write_start..write_start + write_end];
        assert!(
            write_fn.contains("session write requires reserved session_id")
                && write_fn.contains("session_identity_mismatch")
                && !write_fn.contains("active_mut()"),
            "native write must resolve reserved identity, never active_mut(): {write_fn}"
        );
    }

    #[test]
    fn leftover_and_native_apply_share_error_class_and_archive() {
        // Hunt 2: helper vs native — same archive destination (failed vs applied)
        // and same user-visible error class. Do not bikeshed wording.
        let leftover = include_str!("session.rs");
        let native = include_str!("../../src-tauri/src/session_bridge.rs");
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
            .find("fn apply_one_inbox_op(")
            .expect("native apply_one_inbox_op");
        let native_apply_end = native[native_apply..]
            .find("/// Reserve a monotonic generation")
            .expect("end native apply");
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
        // Headless: two boxes → create revolute joint → query → update name/limits.
        // No cad_attach. Uses landed host CreateJointRequestDto / UpdateJointRequestDto.
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
    fn assert_joint_dto_keeps_dirty(host_result: &Value, label: &str) {
        // applyInboxNow: scene+document -> applySolidUpdate (already dirty:true);
        // otherwise refreshAfterInboxApply (dirty:true). Joint DTOs take the
        // second path — the old loadDocument() fallback cleared dirty.
        let is_solid_update =
            host_result.get("scene").is_some() && host_result.get("document").is_some();
        assert!(
            !is_solid_update,
            "{label}: joint DTO must take refreshAfterInboxApply (dirty:true), not loadDocument: {host_result}"
        );
        assert!(
            host_result.get("id").is_some() && host_result.get("kind").is_some(),
            "{label}: expected a joint DTO: {host_result}"
        );
    }

    fn joint_inspect_fields(document: &Value) -> Value {
        // cad_refresh vs cad_load_project_model parity: ids, names, connectors, limits.
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
        // Attached cad_submit: after create AND update, joint is visible and
        // the inbox result is a joint DTO so applyInboxNow keeps dirty:true.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-submit-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        assert_joint_dto_keeps_dirty(&created.host_result, "create");

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
        assert_joint_dto_keeps_dirty(&updated.host_result, "update");

        server.call_tool("cad_refresh", json!({})).unwrap();
        let after_update = server
            .call_tool("assembly_document", json!({}))
            .expect("joints after update apply");
        assert_joint_visible(&after_update, joint_id, "Hinge1Renamed");

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_joint_query_validates_against_advertised_update_schema() {
        // Serialized JointDefinitionDto emits null for absent Option fields.
        // A strict MCP client validates tools/call arguments against tools/list
        // inputSchema, so query → update must accept those nulls.
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
        // Replace-all host: a queried full DTO with only the name changed
        // must keep limits. Omitted optional fields would clear them.
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
        // Host is replace-all, not a patch DTO. Schema-valid updates may omit
        // optional limits; serde default None clears them.
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
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-seq-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_joint_null_fields_schema_and_script_baseline() {
        // Attached create → update with explicit nulls; tools/list schema
        // accepts the queried joint; cad_submit is not a portable script op;
        // after apply+refresh, joints live in the cad_load_project_model baseline.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-adv-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        assert_joint_dto_keeps_dirty(&created.host_result, "create");
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
        assert_joint_dto_keeps_dirty(&updated.host_result, "update-nulls");
        assert_eq!(updated.host_result["id"].as_u64(), Some(joint_id));

        // cad_script is not read-safe while attached. Detach keeps the
        // refresh-seeded baseline; joints live inside model_json.
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_malformed_joint_payload_is_dead_lettered() {
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-dead-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_joint_create_leaves_no_ghost_in_assembly_document() {
        // Dead-lettered create must not mint a joint id on the attached
        // snapshot or the published model.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-ghost-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cad_refresh_and_cad_load_project_model_see_same_joints() {
        // After inbox create+update, attached cad_refresh and a fresh
        // cad_load_project_model of the published model must report the same
        // joint ids, names, connectors, and limits. While attached,
        // cad_load_project_model stays session_read_only.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-parity-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        assert_joint_dto_keeps_dirty(&created.host_result, "create-parity");

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
        assert_joint_dto_keeps_dirty(&updated.host_result, "update-parity");

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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn joint_inbox_on_part_document_is_typed_reject_no_ghost() {
        // cadStore/document is a part (one body, no sibling occurrence). A
        // schema-valid joint inbox op must typed-reject, dead-letter, leave
        // no ghost id, and not stay pending.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-part-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extra_unknown_joint_fields_do_not_change_create_or_update_contract() {
        // Protocol inputSchema is additionalProperties:false. Inbox apply is
        // host serde (no deny_unknown_fields). Extra keys must not become new
        // semantics, a ghost field, or a different joint than the known DTO.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-unknown-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
            schema_accepts(&advertised_create_joint_schema(), &create_args).is_err(),
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn advertised_update_joint_schema() -> Value {
        tool_specs()
            .into_iter()
            .find(|spec| spec.name == "assembly_update_joint")
            .map(|spec| spec.input_schema)
            .expect("assembly_update_joint ToolSpec")
    }

    fn advertised_create_joint_schema() -> Value {
        tool_specs()
            .into_iter()
            .find(|spec| spec.name == "assembly_create_joint")
            .map(|spec| spec.input_schema)
            .expect("assembly_create_joint ToolSpec")
    }

    fn probe_connector_occurrence_ids(unique: &str, body_a: &Value, body_b: &Value) -> (u64, u64) {
        // Auto-promote runs on the first joint create. Probe a separate host so
        // the session snapshot stays unchanged while we learn occurrence ids.
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
        // Wipe is the contract for omitted *optional* fields. Id+name-only is
        // not a rename patch: schema required fields + host serde reject it,
        // so the existing joint is unchanged.
        let spec = tool_specs()
            .into_iter()
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
        schema_accepts(&advertised_update_joint_schema(), &partial)
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
        // Both encodings must be specified: omitted key *and* JSON null.
        // Host serde Option+default treats them the same (clear, not preserve).
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
            &advertised_update_joint_schema(),
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
        // Apply create, then immediately apply update *before* cad_refresh.
        // Attached snapshot stays stale until the one refresh; joint must
        // not be lost, inbox results stay DTO (dirty:true), preview still cleared.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-norefresh-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        assert_joint_dto_keeps_dirty(&created.host_result, "create-before-refresh");
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
        assert_joint_dto_keeps_dirty(&updated.host_result, "update-before-refresh");

        server.call_tool("cad_refresh", json!({})).unwrap();
        let after = server.call_tool("assembly_document", json!({})).unwrap();
        assert_joint_visible(&after, joint_id, "HingeFastRenamed");
        assert_eq!(after["joints"].as_array().map(Vec::len), Some(1));

        let source = include_str!("../../src/store/appStore.ts");
        let start = source
            .find("refreshAfterInboxApply: async (opName)")
            .expect("refreshAfterInboxApply");
        let assembly = source[start..]
            .find("if (opName?.startsWith('assembly_'))")
            .expect("assembly inbox branch");
        let branch = &source[start + assembly..];
        let end = branch
            .find("const doc = await engine.getDocument()")
            .expect("end of targeted assembly refresh");
        let targeted = &branch[..end];
        assert!(
            targeted.contains("jointMotionPreview: null") && targeted.contains("dirty: true"),
            "preview still cleared and dirty true after chained apply"
        );

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_detach_mid_inbox_apply_does_not_fork() {
        // Detach while an inbox create is pending. Apply must run on a
        // separate host (published model), never the detached manager.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-detach-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        assert_joint_dto_keeps_dirty(&created.host_result, "apply-after-detach");

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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_detach_reattach_same_then_apply_does_not_fork() {
        // Pass 2 locked detach-then-apply. This is reattach-then-apply:
        // pending must not run against the reattached manager (fork).
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-reattach-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        assert_joint_dto_keeps_dirty(&created.host_result, "apply-after-reattach");

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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_detach_reattach_other_then_apply_stays_identity_bound() {
        // Pending on A must not apply against a later attach of B.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let session_a = session::test_session_uuid();
        let session_b = loop {
            let candidate = session::test_session_uuid();
            if candidate != session_a {
                break candidate;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-rebind-{session_a}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_wrong_component_is_dead_lettered() {
        // Schema-valid joint against a missing/wrong occurrence (component
        // instance) must be a typed host error, dead-lettered, queue unblocked.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-comp-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        // Missing component/occurrence id.
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
        // Wrong component: occurrence B does not contain body A.
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_whitespace_only_joint_name_is_typed_reject() {
        // Schema minLength:1 accepts "   "; host validate_joint treats trim-empty
        // as a typed reject. Do not invent a max-length or schema pattern.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-ws-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let spec = tool_specs()
            .into_iter()
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_delete_joint_is_available_through_shared_inbox_mapping() {
        assert!(tool_specs()
            .iter()
            .any(|spec| spec.name == "assembly_delete_joint"));
        assert!(nbcad_mcp_mutate::lookup_mutate("assembly_delete_joint").is_some());
        assert_eq!(
            tags_for_tool("assembly_delete_joint").0,
            FocusPack::Assembly
        );
    }
    #[test]
    fn attach_cad_submit_body_delete_of_jointed_feature_removes_joint() {
        // Applied joint, then inbox solid_delete_feature of a connector
        // body. Host cleanup removes the joint; leftover applySolidUpdate
        // (scene+document) must not leave a ghost. Do not invent a
        // delete-joint tool.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-body-del-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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
        assert_joint_dto_keeps_dirty(&created.host_result, "create-before-body-delete");

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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_body_delete_unrelated_feature_keeps_joint() {
        // Delete a body that is not part of the joint. Host cleanup must
        // leave the joint (and its occurrence ids) unchanged.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-unrel-del-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_pending_joint_after_body_delete_is_dead_lettered() {
        // Pending inbox create that names a live occ, then the named body's
        // feature is deleted (same generation), then applyInboxNow. Typed
        // reject, dead-letter, no ghost, seq 2 applies.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-pend-del-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_update_after_joint_body_delete_is_dead_lettered() {
        // Update after the joint's own occurrence/body was body-deleted.
        // Host cleanup already dropped the joint; the pending replace-all
        // must typed-reject, dead-letter, and not resurrect it.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-upd-del-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_inverted_limits_is_typed_reject() {
        // Schema already defines limits as required min/max numbers with no
        // value range. min>max is schema-valid and a typed host reject.
        // Do not invent exclusiveMinimum / maximum.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-limits-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let spec = tool_specs()
            .into_iter()
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_unicode_joint_name_round_trips() {
        // Host accepts any trim-nonempty name. Unicode is schema-valid and
        // must persist through inbox apply + inspect. Do not invent a pattern.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-unicode-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        let scene = write_two_box_session(&unique);
        let bodies = scene["bodies"].as_array().unwrap();
        let body_a = bodies[0].clone();
        let body_b = bodies[1].clone();
        let name = "ヒンジα-1";
        let spec = tool_specs()
            .into_iter()
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_json_wrong_tool_name_is_dead_lettered() {
        // Valid JSON whose name is a known inspect tool or the unknown
        // body-delete cleanup is not an inbox mutate. cad_submit rejects
        // those at submit; a raw inbox file must dead-letter on apply.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-wrong-tool-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_gone_occurrence_after_create_is_dead_lettered() {
        // Create joint (applied), absorb one auto-promoted occurrence away,
        // then inbox update and a second create that name the gone occ.
        // Typed reject, dead-letter, no ghost, later seq still applies.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-gone-occ-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        // Absorb body A: deletes the promoted occurrence and mints a new id.
        // The live joint is rewritten to the new occ; the old id is gone.
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_unknown_joint_id_update_is_dead_lettered() {
        // Replace-all update of a joint id that was never minted: typed host
        // reject, dead-letter, existing joint unchanged, later seq applies.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-unknown-id-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_create_joint_headless_direct_attached_inbox_only() {
        // Headless (not attached): the cad_submit-mapped op still works as a
        // direct tool. cad_submit itself requires attach. While attached,
        // direct call_tool is session_read_only and only inbox is accepted.
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

        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-modes-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
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
        // After inbox create + refresh, assembly_solution must include the
        // jointed occurrences and must not crash if the graph is unsolved.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-sol-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_two_joints_same_occurrence_pair() {
        // Two different joints on the same occurrence pair: host either keeps
        // both or rejects the second with a typed error. Queue must not wedge.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-pair-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_cad_submit_create_update_same_base_generation() {
        // Rapid create + update sharing one base_generation: create applies,
        // leftover same-base update dead-letters with a reason (never silent
        // drop), and a rebased follow-up still applies.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-samebase-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_malformed_inbox_json_is_dead_lettered() {
        // Raw invalid JSON (not just a bad joint DTO) must dead-letter so the
        // next queued mutate can apply.
        let _guard = session::ENV_LOCK.lock().unwrap();
        let unique = session::test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-joint-badjson-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
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

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assembly_update_joint_swapped_connector_occurrence_ids() {
        // Swap-only occurrence ids (bodies stay) is a typed reject. Swapping
        // the whole connectors (bodies + occurrence ids) is legal and query
        // must match.
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

    fn schema_accepts(schema: &Value, value: &Value) -> Result<(), String> {
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
}
