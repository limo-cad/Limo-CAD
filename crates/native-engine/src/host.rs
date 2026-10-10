//! Shared document and exact-geometry host, independent of Bevy windows and transports.
//!
//! Enable `native-occt` for execution. The default feature set permits SDK-free
//! compilation of consumers; it does not supply a geometry kernel.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use limo_cad_cam::CamDocumentDto;
use limo_cad_core::{BodyAppearance, DocumentDto};
use limo_cad_occt::{exact_interference_report, DrawingProjectionRequest, OcctKernel};
use limo_cad_sketch::{
    err_json, host, ok_json, BodyPoseDto, InstanceBodyPoseDto, InterferenceCheckRequestDto,
    SketchDto, SketchManager, SweptCollisionRequestDto,
};
use limo_cad_solid::{
    BodyFeatureRequestDto, DatumPlaneDefinitionDto, DeleteFeatureRequest, EditBodyFeatureRequest,
    EditExtrudeRequest, EditHoleRequest, EditLoftRequest, EditRevolveRequest, EditRibRequest,
    EditSolidChamferRequest, EditSolidFilletRequest, EditSweepRequest, ExtrudeRequest, HoleRequest,
    LoftRequest, ProfileCatalogItemDto, RecomputePlanDto, ReorderFeatureRequest, RevolveRequest,
    RibRequest, SetRollbackRequest, SolidChamferRequest, SolidFilletRequest, SolidSceneDto,
    SolidUpdateDto, StepExportRequest, SweepRequest,
};
use serde::de::DeserializeOwned;

pub const BOOTSTRAP_SESSION_ID: &str = "__bootstrap__";
const MAX_PROJECT_SESSIONS: usize = 128;

/// Model state captured under one document lock for native viewport rendering.
pub type NativeViewportSnapshot = (
    String,
    u64,
    SolidSceneDto,
    Option<SketchDto>,
    Vec<SketchDto>,
    Vec<DatumPlaneDefinitionDto>,
    Vec<ProfileCatalogItemDto>,
    Vec<BodyAppearance>,
    Vec<BodyPoseDto>,
    Vec<InstanceBodyPoseDto>,
);

/// Immutable authored data retained by an in-process viewport frame.
/// Geometry is shared with the engine and never copied to install a frame.
#[derive(Debug, Clone, Default)]
pub struct NativeViewportDocument {
    /// Authored viewport data changes independently of the geometry revision.
    pub metadata_revision: u64,
    pub scene: Arc<SolidSceneDto>,
    pub active_sketch: Option<SketchDto>,
    pub finished_sketches: Vec<SketchDto>,
    pub datum_planes: Vec<DatumPlaneDefinitionDto>,
    pub profile_catalog: Vec<ProfileCatalogItemDto>,
    pub body_appearances: Vec<BodyAppearance>,
}

/// Geometry, authored data and current placements captured under one engine lock.
#[derive(Debug, Clone)]
pub struct NativeViewportFrame {
    pub session_id: String,
    pub geometry_revision: u64,
    pub document: Arc<NativeViewportDocument>,
    pub body_poses: Arc<Vec<BodyPoseDto>>,
    pub instance_body_poses: Arc<Vec<InstanceBodyPoseDto>>,
}

#[path = "local_slicer.rs"]
mod local_slicer;
#[path = "manufacturing.rs"]
mod manufacturing;
#[path = "retention.rs"]
mod retention;
use retention::NativeProject;

#[cfg(test)]
#[path = "viewport_tests.rs"]
mod viewport_tests;

#[cfg(test)]
#[path = "component_edit_tests.rs"]
mod component_edit_tests;

pub use limo_cad_occt::DrawingProjectionBasis;
/// Projection and its actual orthonormal camera axes, computed together.
pub struct ResolvedDrawingProjection {
    pub projection: limo_cad_occt::DrawingProjectionDto,
    pub basis: DrawingProjectionBasis,
}

struct NativeEngine {
    manager: SketchManager,
    kernel: OcctKernel,
    geometry_revision: u64,
    viewport_revision: u64,
    viewport_cache: RefCell<Option<CachedViewport>>,
}

struct CachedViewport {
    document: Arc<NativeViewportDocument>,
    placement: Option<CachedPlacement>,
}

struct CachedPlacement {
    body_poses: Arc<Vec<BodyPoseDto>>,
    instance_body_poses: Arc<Vec<InstanceBodyPoseDto>>,
}

impl NativeEngine {
    fn new() -> Result<Self, String> {
        Ok(Self {
            manager: SketchManager::new(),
            kernel: OcctKernel::new()
                .map_err(|error| format!("native OCCT kernel failed to initialize: {error}"))?,
            geometry_revision: 1,
            viewport_revision: 1,
            viewport_cache: RefCell::new(None),
        })
    }

    fn update(&self) -> SolidUpdateDto {
        SolidUpdateDto {
            document: self.manager.document_dto(),
            scene: self.manager.solid_scene(),
        }
    }

    fn invalidate_viewport(&mut self) {
        self.viewport_revision = self.viewport_revision.wrapping_add(1);
        *self.viewport_cache.get_mut() = None;
    }

    fn invalidate_for_command(&mut self, method: &str) {
        if method.starts_with("drawing_")
            || method.starts_with("cam_")
            || method.starts_with("print_intent_")
            || method.starts_with("print_modifier_")
            || method == "document_set_name"
            || method == "solid_rename_feature"
            || (matches!(method, "set_grid_snap" | "set_grid_step")
                && !self.manager.has_active_sketch())
        {
            return;
        }
        if method.starts_with("assembly_")
            || matches!(
                method,
                "recall_named_view"
                    | "clear_named_view"
                    | "set_named_views"
                    | "upsert_named_view"
                    | "rename_named_view"
                    | "delete_named_view"
                    | "project_set_visibility"
                    | "construction_set_visibility"
            )
        {
            if let Some(cached) = self.viewport_cache.get_mut() {
                cached.placement = None;
            }
        } else {
            self.invalidate_viewport();
        }
    }
}

struct NativeWorkspace {
    verification_owner_id: String,
    active_session_id: String,
    sessions: HashMap<String, NativeProject>,
}

impl NativeWorkspace {
    fn new() -> Self {
        let mut sessions = HashMap::new();
        sessions.insert(
            BOOTSTRAP_SESSION_ID.to_string(),
            NativeProject::Warm(Box::new(
                NativeEngine::new().expect("native OCCT kernel failed to initialize"),
            )),
        );
        Self {
            verification_owner_id: limo_cad_export::slicer_verification::new_verification_owner(),
            active_session_id: BOOTSTRAP_SESSION_ID.to_string(),
            sessions,
        }
    }

    fn verification_owner_key(&self) -> String {
        format!("{}:{}", self.verification_owner_id, self.active_session_id)
    }

    fn active(&self) -> &NativeEngine {
        self.sessions
            .get(&self.active_session_id)
            .expect("active project session missing")
            .warm()
    }

    fn active_mut(&mut self) -> &mut NativeEngine {
        self.sessions
            .get_mut(&self.active_session_id)
            .expect("active project session missing")
            .warm_mut()
    }
}

/// Native application state: the shared Rust document/history manager plus
/// the stateful OCCT B-rep bridge. The whole pair is locked together so a
/// prepare → kernel replay → commit transaction cannot interleave.
pub struct NativeEngineHost {
    inner: Mutex<NativeWorkspace>,
}

impl NativeEngineHost {
    /// Transfer an already recomputed editor model under the caller's document
    /// receipt fence. Moving its kernel retains the B-rep cache and avoids a
    /// second full replay at commit. The prepared state is consumed by this call.
    pub fn install_prepared_document(&self, prepared: &NativeEngineHost) -> Result<(), String> {
        if std::ptr::eq(self, prepared) {
            return Err("An edit needs an isolated model".into());
        }
        let mut current = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
        let next_geometry = current
            .active()
            .geometry_revision
            .checked_add(1)
            .ok_or("Geometry revision exhausted")?;
        let next_viewport = current.active().viewport_revision.wrapping_add(1);
        let mut prepared = prepared
            .inner
            .lock()
            .map_err(|_| "Prepared engine lock poisoned")?;
        if prepared.active_session_id != current.active_session_id {
            return Err("Prepared edit belongs to another document".into());
        }
        let id = current.active_session_id.clone();
        let mut next = prepared
            .sessions
            .remove(&id)
            .ok_or("Prepared edit was already consumed")?;
        next.warm_mut().geometry_revision = next_geometry;
        next.warm_mut().invalidate_viewport();
        next.warm_mut().viewport_revision = next_viewport;
        current.sessions.insert(id, next);
        Ok(())
    }

    pub fn new() -> Self {
        Self {
            inner: Mutex::new(NativeWorkspace::new()),
        }
    }

    /// Native project-session identity currently targeted by engine commands
    /// and inbox apply (`active_mut`).
    pub fn active_project_session_id(&self) -> String {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active_session_id
            .clone()
    }

    /// Associate the engine created during application bootstrap with the
    /// UI's first tab. Repeated binding of the active tab is harmless.
    pub fn bind_project_session(&self, session_id: &str) -> String {
        if let Err(error) = validate_session_id(session_id) {
            return err_json(error);
        }
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        if workspace.active_session_id == session_id {
            return ok_json(());
        }
        if let Some(project) = workspace.sessions.get_mut(session_id) {
            if let Err(error) = project.thaw() {
                return err_json(error);
            }
            workspace.active_session_id = session_id.to_string();
            return ok_json(());
        }
        if workspace.active_session_id != BOOTSTRAP_SESSION_ID || workspace.sessions.len() != 1 {
            return err_json("the bootstrap project session is already bound");
        }
        let engine = workspace
            .sessions
            .remove(BOOTSTRAP_SESSION_ID)
            .expect("bootstrap project session missing");
        workspace.sessions.insert(session_id.to_string(), engine);
        workspace.active_session_id = session_id.to_string();
        ok_json(())
    }

    /// Create and activate a blank, fully retained OCCT project context.
    pub fn create_project_session(&self, session_id: &str) -> String {
        if let Err(error) = validate_session_id(session_id) {
            return err_json(error);
        }
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        if workspace.sessions.contains_key(session_id) {
            return err_json("project session already exists");
        }
        if workspace.sessions.len() >= MAX_PROJECT_SESSIONS {
            return err_json("too many open project tabs");
        }
        let engine = match NativeEngine::new() {
            Ok(engine) => engine,
            Err(error) => return err_json(error),
        };
        let update = engine.update();
        workspace.sessions.insert(
            session_id.to_string(),
            NativeProject::Warm(Box::new(engine)),
        );
        workspace.active_session_id = session_id.to_string();
        ok_json(update)
    }

    /// Activate a retained project, rebuilding a cold native engine atomically.
    /// Failed reconstruction leaves both the cold snapshot and active tab intact.
    pub fn activate_project_session(&self, session_id: &str) -> String {
        if let Err(error) = validate_session_id(session_id) {
            return err_json(error);
        }
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        let Some(project) = workspace.sessions.get_mut(session_id) else {
            return ok_json(false);
        };
        if let Err(error) = project.thaw() {
            return err_json(error);
        }
        workspace.active_session_id = session_id.to_string();
        ok_json(true)
    }

    /// Close an inactive native tab, releasing its warm engine or cold snapshot.
    pub fn drop_project_session(&self, session_id: &str) -> String {
        if let Err(error) = validate_session_id(session_id) {
            return err_json(error);
        }
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        if workspace.active_session_id == session_id {
            return err_json("cannot drop the active project session");
        }
        workspace.sessions.remove(session_id);
        ok_json(())
    }

    pub fn cam_document_snapshot(&self) -> CamDocumentDto {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .cam_document()
    }

    pub fn geometry_revision(&self) -> u64 {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .geometry_revision
    }

    pub fn drawing_snapshot(&self) -> limo_cad_sketch::DrawingDocumentDto {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .drawing_document()
    }

    /// Inspect drawing intent without copying sheets, views or annotations.
    /// The callback runs under the engine guard and must not reenter this host.
    pub fn with_drawing<R>(
        &self,
        inspect: impl FnOnce(&limo_cad_sketch::DrawingDocumentDto) -> R,
    ) -> R {
        let inner = self.inner.lock().expect("engine lock poisoned");
        inspect(inner.active().manager.drawing_document_ref())
    }

    pub fn document_snapshot(&self) -> DocumentDto {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .document_dto()
    }

    /// Inspect the document without copying its feature history or browser tree.
    /// The callback runs under the engine guard and must not reenter this host.
    pub fn with_document<R>(&self, inspect: impl FnOnce(&limo_cad_core::Document) -> R) -> R {
        let inner = self.inner.lock().expect("engine lock poisoned");
        inspect(inner.active().manager.document())
    }

    /// Native frame synchronization only needs the title, not a clone of the
    /// document's feature tree and browser hierarchy.
    pub fn document_name(&self) -> String {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .document()
            .name()
            .to_owned()
    }

    pub fn document_units(&self) -> limo_cad_core::UnitSystem {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .document()
            .settings()
            .units
    }

    /// Undo/Redo button availability does not need to copy feature payloads.
    pub fn document_history_position(&self) -> (usize, usize) {
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let features = workspace.active().manager.document().features();
        (features.rollback_index, features.features.len())
    }

    pub fn is_blank_for_script(&self) -> bool {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .is_blank_for_script()
    }

    /// Clone only the small CAM intent document while holding the engine lock;
    /// expensive voxel work runs later on a background worker without blocking
    /// modeling commands, saves, or viewport synchronization.
    pub fn cam_snapshot(
        &self,
        setup_id: u64,
    ) -> Result<(String, CamDocumentDto, Option<String>), String> {
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let manager = &workspace.active().manager;
        let safety_warning = manager
            .cam_toolpath_safety_warning(setup_id)
            .map_err(|error| error.to_string())?;
        Ok((
            workspace.active_session_id.clone(),
            manager.cam_document(),
            safety_warning,
        ))
    }

    /// One lock acquisition gives the native viewport a coherent model
    /// snapshot. The OCCT triangle buffers stay in Rust and never make a
    /// JSON round-trip through the shared engine dispatch.
    pub fn viewport_snapshot(&self) -> NativeViewportSnapshot {
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let inner = workspace.active();
        let assembly_solution = inner
            .manager
            .presentation_solution()
            .expect("active named view must resolve");
        (
            workspace.active_session_id.clone(),
            inner.geometry_revision,
            inner.manager.solid_scene(),
            inner.manager.active_snapshot(),
            inner.manager.finished_sketches(),
            inner.manager.datum_plane_definitions(),
            inner.manager.profile_catalog(),
            inner.manager.body_appearances(),
            assembly_solution.body_poses,
            assembly_solution.instance_body_poses,
        )
    }

    pub fn engine_call(&self, method: &str, payload: &str) -> String {
        match method {
            "assembly_interference_check" => return self.assembly_interference_check(payload),
            "assembly_evaluate_motion_study" => {
                return self.assembly_evaluate_motion_study(payload)
            }
            "assembly_swept_collision_check" => {
                return self.assembly_swept_collision_check(payload)
            }
            "printer_catalog" => return ok_json(limo_cad_core::embedded_printer_catalog()),
            "print_layout_check" => return self.print_layout_check(payload),
            "solid_export_preflight" => return self.export_preflight(payload),
            "bambu_template_inspect" => return manufacturing::inspect_template(payload),
            "bambu_local_verification_start" => {
                return self.start_local_slicer_verification(payload)
            }
            "bambu_local_verification_poll" => return self.local_slicer_status(payload, false),
            "bambu_local_verification_cancel" => return self.local_slicer_status(payload, true),
            "bambu_project_preview" => return self.bambu_project(payload, true),
            "solid_export_bambu_project" => return self.bambu_project(payload, false),
            "solid_export_3mf" | "solid_export_stl" => {
                use base64::Engine as _;
                let (format, bytes) = if method == "solid_export_3mf" {
                    ("3mf", self.export_3mf(payload))
                } else {
                    ("stl", self.export_stl(payload))
                };
                return match bytes {
                    Ok(bytes) => ok_json(serde_json::json!({
                        "format":format,"encoding":"base64","byte_length":bytes.len(),
                        "bytes_base64":base64::engine::general_purpose::STANDARD.encode(bytes)
                    })),
                    Err(error) => err_json(error),
                };
            }
            "named_view_resolve" => {
                let view: limo_cad_sketch::NamedViewConfigurationDto =
                    match serde_json::from_str(payload) {
                        Ok(view) => view,
                        Err(error) => return err_json(format!("bad request payload: {error}")),
                    };
                let workspace = self.inner.lock().expect("engine lock poisoned");
                return match workspace.active().manager.resolve_named_view(&view) {
                    Ok(solution) => ok_json(solution),
                    Err(error) => err_json(error.to_string()),
                };
            }
            _ => {}
        }
        if method == "drawing_export" {
            return self.drawing_export(payload);
        }
        if method == "drawing_projection" {
            return self.drawing_projection(payload);
        }
        if method == "solid_section_review" {
            return self.section_review(payload);
        }
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        if let Some(response) =
            host::handle_viewport_preview(&mut workspace.active_mut().manager, method, payload)
        {
            return response;
        }
        if let Some(response) = host::handle_read_only(&workspace.active().manager, method, payload)
        {
            return response;
        }
        let verification_owner = workspace.verification_owner_key();
        let inner = workspace.active_mut();
        inner.invalidate_for_command(method);
        let result = host::handle(&mut inner.manager, method, payload);
        let succeeded = serde_json::from_str::<serde_json::Value>(&result)
            .ok()
            .is_some_and(|reply| reply["ok"] == true);
        if succeeded && method == "solid_rename_feature" {
            let renamed_sketch = serde_json::from_str::<serde_json::Value>(payload)
                .ok()
                .and_then(|request| request["feature_id"].as_u64())
                .is_some_and(|id| {
                    inner
                        .manager
                        .document()
                        .features()
                        .features
                        .iter()
                        .any(|feature| {
                            feature.id.0 == id && feature.kind == limo_cad_core::FeatureKind::Sketch
                        })
                });
            if renamed_sketch {
                inner.invalidate_viewport();
            }
        }
        if succeeded
            && matches!(
                method,
                "datum_plane_create"
                    | "datum_plane_edit"
                    | "recall_named_view"
                    | "clear_named_view"
                    | "set_named_views"
                    | "upsert_named_view"
                    | "rename_named_view"
                    | "delete_named_view"
                    | "project_set_visibility"
            )
        {
            inner.geometry_revision = inner.geometry_revision.wrapping_add(1);
        }
        if succeeded {
            let _ = limo_cad_export::slicer_verification::local_slicer_service()
                .observe_owned_model(&verification_owner, || {
                    inner
                        .manager
                        .export_project_model()
                        .map_err(|error| error.to_string())
                });
        }
        result
    }

    /// Apply one MCP mutate using the shared name→method map encoding.
    /// Direct tools go through `host::handle`; solid-replay tools prepare,
    /// recompute, and commit on the live kernel (same path as IPC).
    pub fn apply_encoded_mutate(&self, method: &str, payload: &str, solid: bool) -> String {
        if !solid {
            return self.engine_call(method, payload);
        }
        match method {
            "solid_prepare_body_feature" => return self.solid_body_feature(payload),
            "solid_prepare_edit_body_feature" => return self.solid_edit_body_feature(payload),
            _ => {}
        }
        self.execute(|manager| {
            let raw = host::handle(manager, method, payload);
            let envelope: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
                limo_cad_sketch::SessionError::Solid(format!("invalid engine response: {error}"))
            })?;
            if envelope.get("ok").and_then(|value| value.as_bool()) != Some(true) {
                let message = envelope
                    .get("error")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown Limo CAD engine error")
                    .to_string();
                return Err(limo_cad_sketch::SessionError::Solid(message));
            }
            let value = envelope
                .get("value")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            serde_json::from_value(value).map_err(|error| {
                limo_cad_sketch::SessionError::Solid(format!(
                    "engine returned an invalid recompute plan: {error}"
                ))
            })
        })
    }

    pub fn solid_extrude(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: ExtrudeRequest| {
            manager.prepare_extrude(request)
        })
    }

    pub fn solid_edit_extrude(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditExtrudeRequest| {
            manager.prepare_edit_extrude(request)
        })
    }

    pub fn solid_revolve(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: RevolveRequest| {
            manager.prepare_revolve(request)
        })
    }

    pub fn solid_edit_revolve(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditRevolveRequest| {
            manager.prepare_edit_revolve(request)
        })
    }

    pub fn solid_sweep(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: SweepRequest| {
            manager.prepare_sweep(request)
        })
    }

    pub fn solid_edit_sweep(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditSweepRequest| {
            manager.prepare_edit_sweep(request)
        })
    }

    pub fn solid_loft(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: LoftRequest| {
            manager.prepare_loft(request)
        })
    }

    pub fn solid_edit_loft(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditLoftRequest| {
            manager.prepare_edit_loft(request)
        })
    }

    pub fn solid_rib(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: RibRequest| {
            manager.prepare_rib(request)
        })
    }

    pub fn solid_edit_rib(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditRibRequest| {
            manager.prepare_edit_rib(request)
        })
    }

    pub fn solid_fillet(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: SolidFilletRequest| {
            manager.prepare_solid_fillet(request)
        })
    }

    pub fn solid_edit_fillet(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditSolidFilletRequest| {
            manager.prepare_edit_solid_fillet(request)
        })
    }

    pub fn solid_chamfer(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: SolidChamferRequest| {
            manager.prepare_solid_chamfer(request)
        })
    }

    pub fn solid_edit_chamfer(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditSolidChamferRequest| {
            manager.prepare_edit_solid_chamfer(request)
        })
    }

    pub fn solid_hole(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: HoleRequest| {
            manager.prepare_hole(request)
        })
    }

    pub fn solid_edit_hole(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditHoleRequest| {
            manager.prepare_edit_hole(request)
        })
    }

    pub fn solid_body_feature(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: BodyFeatureRequestDto| {
            validate_step_import(&request)?;
            manager.prepare_body_feature(request)
        })
    }

    pub fn solid_edit_body_feature(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditBodyFeatureRequest| {
            validate_step_import(&request.feature)?;
            manager.prepare_edit_body_feature(request)
        })
    }

    pub fn solid_recompute(&self) -> String {
        self.execute(|manager| manager.prepare_recompute())
    }

    pub fn solid_set_rollback(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: SetRollbackRequest| {
            manager.prepare_set_rollback(request)
        })
    }

    pub fn solid_delete_feature(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: DeleteFeatureRequest| {
            manager.prepare_delete_feature(request)
        })
    }

    pub fn solid_reorder_feature(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: ReorderFeatureRequest| {
            manager.prepare_reorder_feature(request)
        })
    }

    pub fn project_load(&self, payload: &str) -> String {
        let model_json: String = match serde_json::from_str(payload) {
            Ok(model) => model,
            Err(error) => {
                return unchanged_project_load_error(format!("bad request payload: {error}"))
            }
        };
        self.execute_with_prepare_error(
            |manager| manager.prepare_load_project(model_json),
            unchanged_project_load_error,
        )
    }

    pub fn project_new(&self) -> String {
        self.execute(SketchManager::prepare_new_project)
    }

    pub fn assembly_interference_check(&self, payload: &str) -> String {
        let request: InterferenceCheckRequestDto = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        let workspace = match self.inner.lock() {
            Ok(workspace) => workspace,
            Err(_) => return err_json("engine lock poisoned"),
        };
        let inner = workspace.active();
        let solution = inner.manager.assembly_solution();
        if !solution.solved {
            return err_json("Cannot inspect interference in an unsolved assembly");
        }
        match exact_interference_report(
            &inner.kernel,
            inner.manager.solid_scene_ref(),
            &solution.instance_body_poses,
            &request,
        ) {
            Ok(report) => ok_json(report),
            Err(error) => err_json(error),
        }
    }

    pub fn assembly_evaluate_motion_study(&self, payload: &str) -> String {
        let request = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        let workspace = match self.inner.lock() {
            Ok(workspace) => workspace,
            Err(_) => return err_json("engine lock poisoned"),
        };
        let inner = workspace.active();
        match limo_cad_occt::evaluate_motion_study(&inner.manager, &inner.kernel, &request) {
            Ok(result) => ok_json(result),
            Err(error) => err_json(error),
        }
    }

    pub fn assembly_swept_collision_check(&self, payload: &str) -> String {
        let request: SweptCollisionRequestDto = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        let workspace = match self.inner.lock() {
            Ok(workspace) => workspace,
            Err(_) => return err_json("engine lock poisoned"),
        };
        let inner = workspace.active();
        match limo_cad_occt::exact_swept_collision_check(&inner.manager, &inner.kernel, &request) {
            Ok(report) => ok_json(report),
            Err(error) => err_json(error),
        }
    }

    pub fn export_step(&self, payload: &str) -> Result<Vec<u8>, String> {
        let request: StepExportRequest = serde_json::from_str(payload)
            .map_err(|error| format!("bad request payload: {error}"))?;
        let workspace = self
            .inner
            .lock()
            .map_err(|_| "engine lock poisoned".to_string())?;
        let inner = workspace.active();
        if request.expected_model_json.is_some() {
            limo_cad_solid::check_export_model_snapshot(
                request.expected_model_json.as_deref(),
                &inner
                    .manager
                    .export_project_model()
                    .map_err(|e| e.to_string())?,
            )?;
        }
        if !inner.manager.solid_scene_ref().errors.is_empty() {
            return Err("Resolve timeline errors before exporting STEP.".to_string());
        }
        inner
            .kernel
            .export_step(&request)
            .map_err(|error| error.to_string())
    }

    pub fn drawing_export(&self, payload: &str) -> String {
        self.drawing_export_observing(payload, |_, _| {})
    }

    /// Observe the projections the normal exporter actually uses, including
    /// resolved derived-view requests. The caller may present this linework;
    /// it never starts a second projection or changes the export response.
    pub fn drawing_export_observing(
        &self,
        payload: &str,
        mut completed: impl FnMut(&DrawingProjectionRequest, &limo_cad_occt::DrawingProjectionDto),
    ) -> String {
        let request: limo_cad_occt::drawing_export::DrawingExportRequest =
            match serde_json::from_str(payload) {
                Ok(request) => request,
                Err(error) => return err_json(format!("bad request payload: {error}")),
            };
        let workspace = match self.inner.lock() {
            Ok(workspace) => workspace,
            Err(_) => return err_json("engine lock poisoned"),
        };
        let inner = workspace.active();
        let scene = inner.manager.solid_scene_ref();
        let assembly = inner.manager.assembly_document_ref();
        let content = limo_cad_occt::drawing_export::export_sheet_with_units(
            inner.manager.drawing_document_ref(),
            scene,
            assembly,
            &request,
            inner.manager.document().settings().units,
            |r| {
                let projection = limo_cad_occt::project_drawing(&inner.kernel, scene, assembly, r)
                    .map_err(|e| e.to_string())?;
                completed(r, &projection);
                Ok(projection)
            },
        );
        match content {
            Ok(content) => ok_json(
                serde_json::json!({"format":request.format,"encoding":"utf8","content":content,"sheet_id":request.sheet_id}),
            ),
            Err(error) => err_json(error),
        }
    }

    /// Project stored view intent against current topology. Derived views need
    /// the complete sheet to resolve parent bases, cutting planes and depth.
    pub fn project_sheet_view(
        &self,
        view: &limo_cad_sketch::DrawingViewDto,
        sheet_views: &[limo_cad_sketch::DrawingViewDto],
    ) -> Result<limo_cad_occt::DrawingProjectionDto, String> {
        self.project_sheet_view_resolved(view, sheet_views)
            .map(|result| result.projection)
    }

    pub fn project_sheet_view_resolved(
        &self,
        view: &limo_cad_sketch::DrawingViewDto,
        sheet_views: &[limo_cad_sketch::DrawingViewDto],
    ) -> Result<ResolvedDrawingProjection, String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let scene = inner.manager.solid_scene_ref();
        if !scene.errors.is_empty() {
            return Err("Resolve timeline errors before generating a drawing view.".into());
        }
        let assembly = inner.manager.assembly_document_ref();
        let request =
            limo_cad_occt::drawing_export::projection_request(view, sheet_views, scene, assembly)?;
        let basis = limo_cad_occt::drawing_projection_basis(request.direction, request.up)
            .map_err(|error| error.to_string())?;
        let projection = limo_cad_occt::project_drawing(&inner.kernel, scene, assembly, &request)
            .map_err(|error| error.to_string())?;
        Ok(ResolvedDrawingProjection { projection, basis })
    }

    /// Disposable source-view marks use the same current topology resolver as
    /// export. Cached projections are borrowed; no second projection is run.
    pub fn section_source_graphics<'a>(
        &self,
        sheet: &limo_cad_sketch::DrawingSheetDto,
        projection: impl Fn(u64) -> Option<&'a limo_cad_occt::DrawingProjectionDto>,
        budget: &mut limo_cad_occt::drawing_export::PaperGraphicsBudget,
    ) -> Result<Vec<limo_cad_occt::drawing_export::PaperPrimitive>, String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let scene = inner.manager.solid_scene_ref();
        if !scene.errors.is_empty() {
            return Err("Resolve timeline errors before generating a drawing view.".into());
        }
        let assembly = inner.manager.assembly_document_ref();
        let mut graphics = Vec::new();
        for view in &sheet.views {
            if view.derivation.is_some() {
                let marks = limo_cad_occt::drawing_export::derived_source_graphics(
                    view,
                    sheet,
                    &projection,
                    scene,
                    assembly,
                    budget,
                )?;
                budget.append(&mut graphics, marks)?;
            }
        }
        Ok(graphics)
    }

    pub fn section_review(&self, payload: &str) -> String {
        let result = (|| {
            let request: limo_cad_occt::section_review::SectionReviewRequest =
                serde_json::from_str(payload).map_err(|e| format!("bad request payload: {e}"))?;
            let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
            let inner = workspace.active();
            limo_cad_occt::section_review::inspect(
                &inner.kernel,
                inner.manager.solid_scene_ref(),
                &request,
            )
            .map_err(|error| error.to_string())
        })();
        match result {
            Ok(review) => ok_json(review),
            Err(error) => err_json(error),
        }
    }

    pub fn drawing_projection(&self, payload: &str) -> String {
        let request: DrawingProjectionRequest = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        let workspace = match self.inner.lock() {
            Ok(workspace) => workspace,
            Err(_) => return err_json("engine lock poisoned"),
        };
        let inner = workspace.active();
        let scene = inner.manager.solid_scene_ref();
        if !scene.errors.is_empty() {
            return err_json("Resolve timeline errors before generating a drawing view.");
        }
        match limo_cad_occt::project_drawing(
            &inner.kernel,
            scene,
            inner.manager.assembly_document_ref(),
            &request,
        ) {
            Ok(projection) => ok_json(projection),
            Err(error) => err_json(error.to_string()),
        }
    }

    pub fn export_stl(&self, payload: &str) -> Result<Vec<u8>, String> {
        let request: limo_cad_export::MeshExportRequest = serde_json::from_str(payload)
            .map_err(|error| format!("bad request payload: {error}"))?;
        if request.scope == limo_cad_export::MeshExportScope::Definition
            && request
                .named_view
                .as_deref()
                .is_some_and(|name| !name.is_empty())
        {
            return Err(
                "A named layout requires assembly scope; definition scope uses source coordinates."
                    .into(),
            );
        }
        let workspace = self
            .inner
            .lock()
            .map_err(|_| "engine lock poisoned".to_string())?;
        let inner = workspace.active();
        if request.expected_model_json.is_some() {
            request
                .check_model_snapshot(
                    &inner
                        .manager
                        .export_project_model()
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
        }
        if !inner.manager.solid_scene_ref().errors.is_empty() {
            return Err("Resolve timeline errors before exporting STL.".to_string());
        }
        let scene = inner.manager.solid_scene_ref();
        scene.require_complete_display_mesh(&request.body_ids)?;
        let mut meshes = inner
            .kernel
            .tessellate_bodies(&request)
            .map_err(|error| error.to_string())?;
        for mesh in &mut meshes {
            if let Some(body) = scene.bodies.iter().find(|body| body.id == mesh.body_id) {
                mesh.name = body.name.clone();
            }
        }
        let solution = if request.scope == limo_cad_export::MeshExportScope::Definition {
            inner.manager.assembly_solution()
        } else {
            inner
                .manager
                .export_view_solution(request.named_view.as_deref())
                .map_err(|error| error.to_string())?
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
        let meshes = limo_cad_export::prepare_export_meshes(&meshes, &instances, request.scope)
            .map_err(|e| e.to_string())?;
        limo_cad_export::write_stl(&meshes).map_err(|error| error.to_string())
    }

    pub fn export_3mf(&self, payload: &str) -> Result<Vec<u8>, String> {
        let mut request: limo_cad_export::MeshExportRequest = serde_json::from_str(payload)
            .map_err(|error| format!("bad request payload: {error}"))?;
        if request.scope == limo_cad_export::MeshExportScope::Definition
            && request
                .named_view
                .as_deref()
                .is_some_and(|name| !name.is_empty())
        {
            return Err(
                "A named layout requires assembly scope; definition scope uses source coordinates."
                    .into(),
            );
        }
        let workspace = self
            .inner
            .lock()
            .map_err(|_| "engine lock poisoned".to_string())?;
        let inner = workspace.active();
        if request.expected_model_json.is_some() {
            request
                .check_model_snapshot(
                    &inner
                        .manager
                        .export_project_model()
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
        }
        if !inner.manager.solid_scene_ref().errors.is_empty() {
            return Err("Resolve timeline errors before exporting 3MF.".to_string());
        }
        let scene = inner.manager.solid_scene_ref();
        scene.require_complete_display_mesh(&request.body_ids)?;
        let appearances = inner.manager.body_appearances();
        let mut meshes = inner
            .kernel
            .tessellate_bodies(&request)
            .map_err(|error| error.to_string())?;
        for mesh in &mut meshes {
            if let Some(body) = scene.bodies.iter().find(|body| body.id == mesh.body_id) {
                mesh.name = body.name.clone();
            }
        }
        let solution = if request.scope == limo_cad_export::MeshExportScope::Definition {
            inner.manager.assembly_solution()
        } else {
            inner
                .manager
                .export_view_solution(request.named_view.as_deref())
                .map_err(|error| error.to_string())?
        };
        if request.scope == limo_cad_export::MeshExportScope::Assembly && !solution.solved {
            return Err("Resolve assembly errors before mesh export.".into());
        }
        if request.scope == limo_cad_export::MeshExportScope::Assembly
            && request.print_bed.is_none()
        {
            request.print_bed = Some(
                inner
                    .manager
                    .export_print_bed(request.named_view.as_deref())
                    .map_err(|e| e.to_string())?,
            );
        }
        limo_cad_export::write_3mf_scene(
            &meshes,
            &appearances,
            &request,
            &inner.manager.assembly_document_ref().component_structure,
            &solution,
        )
        .map_err(|error| error.to_string())
    }

    /// Check saved or unsaved layouts under the same workspace lock as export.
    /// Diagnostics and proposals never modify a view or solid definition.
    pub fn print_layout_check(&self, payload: &str) -> String {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            #[serde(default)]
            view: Option<limo_cad_sketch::NamedViewConfigurationDto>,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            bed: Option<limo_cad_core::PrintBedDto>,
            #[serde(default)]
            body_ids: Vec<limo_cad_core::BodyId>,
            #[serde(default)]
            expected_model_json: Option<String>,
        }
        let request: Request = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        if request.view.is_some() && request.name.is_some() {
            return err_json("Choose either a draft view or a saved view name");
        }
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let export = limo_cad_export::MeshExportRequest {
            named_view: request.name,
            print_bed: request.bed,
            body_ids: request.body_ids,
            expected_model_json: request.expected_model_json,
            ..Default::default()
        };
        let result = check_layout_request(workspace.active(), &export)
            .and_then(|_| check_native_layout(workspace.active(), &export, request.view.as_ref()));
        match result {
            Ok(report) => ok_json(report),
            Err(error) => err_json(error),
        }
    }

    /// Serve attached MCP preflight from its owning native workspace, retaining
    /// transient recalled placement and checking the same snapshot as export.
    pub fn export_preflight(&self, payload: &str) -> String {
        let request: limo_cad_export::MeshExportRequest = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let inner = workspace.active();
        let result = (|| -> Result<_, String> {
            check_layout_request(inner, &request)?;
            let scene = inner.manager.solid_scene_ref();
            let errors: Vec<_> = scene
                .errors
                .iter()
                .map(|e| format!("feature {}: {}", e.feature_id.0, e.message))
                .collect();
            let body_ids: Vec<_> = scene.bodies.iter().map(|b| b.id.0).collect();
            let appearing: Vec<_> = inner
                .manager
                .body_appearances()
                .iter()
                .map(|a| a.body_id.0)
                .collect();
            let missing: Vec<_> = body_ids
                .iter()
                .copied()
                .filter(|id| !appearing.contains(id))
                .collect();
            let ok = errors.is_empty() && !body_ids.is_empty();
            let mut result = serde_json::json!({
                "ok":ok,"body_count":body_ids.len(),"body_ids":body_ids,"timeline_errors":errors,
                "appearances_assigned":appearing.len(),"bodies_missing_appearance":missing,
                "hints": if ok { vec!["Ready for solid_export_3mf (preferred) or solid_export_stl / solid_export_step."] }
                    else { vec!["Fix timeline_errors before export.","Empty documents cannot export meshes.",
                        "Optional: set_body_appearance / material_catalog for colored 3MF."] }
            });
            result["print_intent"] = serde_json::to_value(
                inner
                    .manager
                    .effective_print_intent(
                        request.body_ids.clone(),
                        Some(limo_cad_core::PrintIntentTargetDto::Portable),
                    )
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if ok {
                let (meshes, solution, bed) = native_layout_inputs(inner, &request, None)?;
                let layout = limo_cad_export::analyze_print_layout(
                    &meshes,
                    &inner.manager.assembly_document_ref().component_structure,
                    &solution,
                    &bed,
                )
                .map_err(|e| e.to_string())?;
                let effective = inner
                    .manager
                    .effective_print_intent(
                        request.body_ids.clone(),
                        Some(limo_cad_core::PrintIntentTargetDto::Portable),
                    )
                    .map_err(|e| e.to_string())?;
                result["manufacturing"] = serde_json::to_value(
                    limo_cad_export::manufacturing_report::manufacturing_preflight_report(
                        &meshes,
                        &inner.manager.body_appearances(),
                        &inner.manager.assembly_document_ref().component_structure,
                        &solution,
                        &inner.manager.print_intent(),
                        &effective,
                    )
                    .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if layout.printable_instances == 0 {
                    result["ok"] = serde_json::json!(false);
                    result["hints"] =
                        serde_json::json!(["No visible occurrences are included in this export."]);
                }
                result["layout"] = serde_json::to_value(layout).map_err(|e| e.to_string())?;
            }
            Ok(result)
        })();
        match result {
            Ok(result) => ok_json(result),
            Err(error) => err_json(error),
        }
    }

    fn with_request<T: DeserializeOwned>(
        &self,
        payload: &str,
        prepare: impl FnOnce(
            &mut SketchManager,
            T,
        ) -> Result<RecomputePlanDto, limo_cad_sketch::SessionError>,
    ) -> String {
        let request = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        self.execute(|manager| prepare(manager, request))
    }

    fn execute(
        &self,
        prepare: impl FnOnce(
            &mut SketchManager,
        ) -> Result<RecomputePlanDto, limo_cad_sketch::SessionError>,
    ) -> String {
        self.execute_with_prepare_error(prepare, err_json)
    }

    fn execute_with_prepare_error(
        &self,
        prepare: impl FnOnce(
            &mut SketchManager,
        ) -> Result<RecomputePlanDto, limo_cad_sketch::SessionError>,
        reject_prepare: impl FnOnce(String) -> String,
    ) -> String {
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        let inner = workspace.active_mut();
        inner.invalidate_viewport();
        let plan = match prepare(&mut inner.manager) {
            Ok(plan) => plan,
            Err(error) => return reject_prepare(error.to_string()),
        };
        let transaction_id = plan.transaction_id;
        let queries = inner.manager.history_support_queries();
        let (kernel_scene, verified) = match inner.kernel.recompute_with_supports(&plan, &queries) {
            Ok(scene) => scene,
            Err(error) => {
                inner.manager.cancel_solid_recompute(transaction_id);
                return err_json(error.to_string());
            }
        };
        match inner.manager.commit_solid_with_verified_supports(
            limo_cad_solid::CommitKernelRequest {
                transaction_id,
                scene: kernel_scene,
            },
            &verified,
        ) {
            Ok(update) => {
                inner.geometry_revision = inner.geometry_revision.wrapping_add(1);
                ok_json(update)
            }
            Err(error) => err_json(error.to_string()),
        }
    }
}

impl Default for NativeEngineHost {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeEngineHost {
    /// Retain just the evaluated geometry for a synchronous in-process reader.
    pub fn solid_scene_snapshot(&self) -> Arc<SolidSceneDto> {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .solid_scene_snapshot()
    }

    /// Capture a coherent frame while sharing the engine's evaluated geometry.
    /// Transport consumers can continue requesting an owned DTO snapshot.
    pub fn viewport_frame(&self) -> NativeViewportFrame {
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let inner = workspace.active();
        let mut cache = inner.viewport_cache.borrow_mut();
        let cached = cache.get_or_insert_with(|| CachedViewport {
            document: Arc::new(NativeViewportDocument {
                metadata_revision: inner.viewport_revision,
                scene: inner.manager.solid_scene_snapshot(),
                active_sketch: inner.manager.active_snapshot(),
                finished_sketches: inner.manager.finished_sketches(),
                datum_planes: inner.manager.datum_plane_definitions(),
                profile_catalog: inner.manager.profile_catalog(),
                body_appearances: inner.manager.body_appearances(),
            }),
            placement: None,
        });
        let placement = cached.placement.get_or_insert_with(|| {
            let assembly = inner
                .manager
                .presentation_solution()
                .expect("active named view must resolve");
            CachedPlacement {
                body_poses: Arc::new(assembly.body_poses),
                instance_body_poses: Arc::new(assembly.instance_body_poses),
            }
        });
        NativeViewportFrame {
            session_id: workspace.active_session_id.clone(),
            geometry_revision: inner.geometry_revision,
            document: Arc::clone(&cached.document),
            body_poses: Arc::clone(&placement.body_poses),
            instance_body_poses: Arc::clone(&placement.instance_body_poses),
        }
    }
}

fn check_layout_request(
    inner: &NativeEngine,
    request: &limo_cad_export::MeshExportRequest,
) -> Result<(), String> {
    if request.scope != limo_cad_export::MeshExportScope::Assembly {
        return Err("Print layout checks require assembly scope.".into());
    }
    if request.expected_model_json.is_some() {
        request
            .check_model_snapshot(
                &inner
                    .manager
                    .export_project_model()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn check_native_layout(
    inner: &NativeEngine,
    request: &limo_cad_export::MeshExportRequest,
    draft: Option<&limo_cad_sketch::NamedViewConfigurationDto>,
) -> Result<limo_cad_export::PrintLayoutReport, String> {
    let (meshes, solution, bed) = native_layout_inputs(inner, request, draft)?;
    limo_cad_export::analyze_print_layout(
        &meshes,
        &inner.manager.assembly_document_ref().component_structure,
        &solution,
        &bed,
    )
    .map_err(|e| e.to_string())
}

fn native_layout_inputs(
    inner: &NativeEngine,
    request: &limo_cad_export::MeshExportRequest,
    draft: Option<&limo_cad_sketch::NamedViewConfigurationDto>,
) -> Result<
    (
        Vec<limo_cad_export::TriangleMesh>,
        limo_cad_sketch::AssemblySolutionDto,
        limo_cad_core::PrintBedDto,
    ),
    String,
> {
    if !inner.manager.solid_scene_ref().errors.is_empty() {
        return Err("Resolve timeline errors before checking the print layout.".into());
    }
    let solution = match draft {
        Some(view) => inner.manager.resolve_named_view(view),
        None => inner
            .manager
            .export_view_solution(request.named_view.as_deref()),
    }
    .map_err(|e| e.to_string())?;
    let bed = match &request.print_bed {
        Some(bed) => bed.clone(),
        None => match draft {
            Some(view) => view.print_bed.clone(),
            None => inner
                .manager
                .export_print_bed(request.named_view.as_deref())
                .map_err(|e| e.to_string())?,
        },
    };
    inner
        .manager
        .solid_scene_ref()
        .require_complete_display_mesh(&request.body_ids)?;
    let mut meshes = inner
        .kernel
        .tessellate_bodies(request)
        .map_err(|e| e.to_string())?;
    let scene = inner.manager.solid_scene_ref();
    for mesh in &mut meshes {
        if let Some(body) = scene.bodies.iter().find(|body| body.id == mesh.body_id) {
            mesh.name = body.name.clone();
        }
    }
    Ok((meshes, solution, bed))
}

/// Reject unreadable external geometry before allocating any live history or
/// changing the live B-rep cache. Normal parametric recompute deliberately keeps
/// per-feature errors, which must not turn a failed file import into success.
fn validate_step_import(
    request: &BodyFeatureRequestDto,
) -> Result<(), limo_cad_sketch::SessionError> {
    if !matches!(request, BodyFeatureRequestDto::ImportStep(_)) {
        return Ok(());
    }
    let plan = SketchManager::new().prepare_body_feature(request.clone())?;
    let mut kernel = OcctKernel::new()
        .map_err(|error| limo_cad_sketch::SessionError::Solid(error.to_string()))?;
    let scene = kernel
        .recompute(&plan)
        .map_err(|error| limo_cad_sketch::SessionError::Solid(error.to_string()))?;
    if let Some(error) = scene.errors.first() {
        return Err(limo_cad_sketch::SessionError::Solid(error.message.clone()));
    }
    if scene.bodies.is_empty() {
        return Err(limo_cad_sketch::SessionError::Solid(
            "STEP import produced no bodies".into(),
        ));
    }
    Ok(())
}

/// Only parsing/prepare can prove that neither model nor kernel was replaced.
/// Recompute may mutate kernel bodies before failing; its errors stay unverified.
fn unchanged_project_load_error(message: String) -> String {
    serde_json::json!({
        "ok": false,
        "error": message,
        "data": {"project_load_state": "unchanged"}
    })
    .to_string()
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty() || session_id.len() > 128 {
        return Err("invalid project session id".to_string());
    }
    if session_id == BOOTSTRAP_SESSION_ID {
        return Err("reserved project session id".to_string());
    }
    Ok(())
}

#[cfg(all(test, feature = "native-occt"))]
#[path = "drawing_export_tests.rs"]
mod drawing_export_tests;

#[cfg(all(test, feature = "native-occt"))]
#[path = "print_layout_tests.rs"]
mod print_layout_tests;

#[cfg(all(test, feature = "native-occt"))]
mod tests {
    use super::*;

    fn value(json: String) -> serde_json::Value {
        let envelope: serde_json::Value = serde_json::from_str(&json).expect("valid envelope");
        assert_eq!(envelope["ok"], true, "engine error: {envelope}");
        envelope["value"].clone()
    }

    #[test]
    fn rejected_project_prepare_marks_unchanged_and_preserves_native_export() {
        let state = NativeEngineHost::new();
        value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
        value(state.engine_call(
            "add_rectangle",
            r#"{
            "mode":"two_point","p1":{"x":-10.0,"y":-10.0},
            "p2":{"x":10.0,"y":10.0},"ctrl_held":false
        }"#,
        ));
        value(state.engine_call("end_sketch", ""));
        value(state.solid_extrude(
            r#"{
            "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
            "extent":{"type":"distance","distance":10.0},"taper_angle_deg":0.0,
            "flip":false,"target_body_ids":[]
        }"#,
        ));
        let model = value(state.engine_call("project_export_model", ""));
        let request = serde_json::json!({"expected_model_json":model}).to_string();
        let mesh = state.export_stl(&request).unwrap();
        assert!(!mesh.is_empty());
        let mut invalid: serde_json::Value = serde_json::from_str(model.as_str().unwrap()).unwrap();
        invalid["schema_version"] = serde_json::json!(999);
        for payload in [
            "{}".to_string(),
            serde_json::to_string(&invalid.to_string()).unwrap(),
        ] {
            let error: serde_json::Value =
                serde_json::from_str(&state.project_load(&payload)).unwrap();
            assert_eq!(error["ok"], false);
            assert_eq!(error["data"]["project_load_state"], "unchanged");
            assert!(error["error"]
                .as_str()
                .is_some_and(|message| !message.is_empty()));
            assert_eq!(value(state.engine_call("project_export_model", "")), model);
            assert_eq!(state.export_stl(&request).unwrap(), mesh);
        }
        let ordinary: serde_json::Value = serde_json::from_str(&state.solid_extrude("{}")).unwrap();
        assert!(ordinary.get("data").is_none());
    }

    #[test]
    fn rejected_step_import_preserves_model_history_and_live_kernel_for_ipc_and_inbox() {
        use base64::Engine as _;
        let state = NativeEngineHost::new();
        value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
        value(state.engine_call("add_rectangle", r#"{"mode":"two_point","p1":{"x":0.0,"y":0.0},"p2":{"x":20.0,"y":10.0},"ctrl_held":false}"#));
        value(state.engine_call("end_sketch", ""));
        value(state.solid_extrude(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3.0},"taper_angle_deg":0.0,"flip":false,"target_body_ids":[]}"#));
        let valid_step = state.export_step("{}").unwrap();
        for edit in [false, true] {
            if edit {
                let import = serde_json::json!({"type":"import_step","request":{
                    "file_name":"existing.step",
                    "data_base64":base64::engine::general_purpose::STANDARD.encode(&valid_step)
                }});
                value(state.apply_encoded_mutate(
                    "solid_prepare_body_feature",
                    &import.to_string(),
                    true,
                ));
            }
            let feature_id = state.document_snapshot().features.last().unwrap().id;
            let model = value(state.engine_call("project_export_model", ""));
            let revision = state.geometry_revision();
            let scene = serde_json::to_value(state.viewport_snapshot().2).unwrap();
            let mesh = state.export_stl("{}").unwrap();
            for encoded in [false, true] {
                for source in [
                    "not a STEP file",
                    "ISO-10303-21;\nHEADER;ENDSEC;\nDATA;ENDSEC;\nEND-ISO-10303-21;",
                ] {
                    let import = serde_json::json!({"type":"import_step","request":{
                        "file_name":"invalid.step",
                        "data_base64":base64::engine::general_purpose::STANDARD.encode(source)
                    }});
                    let payload = if edit {
                        serde_json::json!({"feature_id":feature_id,"feature":import})
                    } else {
                        import
                    }
                    .to_string();
                    let response = match (encoded, edit) {
                        (true, false) => {
                            state.apply_encoded_mutate("solid_prepare_body_feature", &payload, true)
                        }
                        (true, true) => state.apply_encoded_mutate(
                            "solid_prepare_edit_body_feature",
                            &payload,
                            true,
                        ),
                        (false, false) => state.solid_body_feature(&payload),
                        (false, true) => state.solid_edit_body_feature(&payload),
                    };
                    let error: serde_json::Value = serde_json::from_str(&response).unwrap();
                    assert_eq!(error["ok"], false, "{error}");
                    assert!(error["error"]
                        .as_str()
                        .is_some_and(|error| !error.is_empty()));
                    assert_eq!(value(state.engine_call("project_export_model", "")), model);
                    assert_eq!(state.geometry_revision(), revision);
                    assert_eq!(
                        serde_json::to_value(state.viewport_snapshot().2).unwrap(),
                        scene
                    );
                    assert_eq!(state.export_stl("{}").unwrap(), mesh);
                }
            }
        }
    }

    #[test]
    fn project_sessions_retain_and_release_independent_documents() {
        let state = NativeEngineHost::new();
        value(state.bind_project_session("tab-a"));
        value(state.engine_call("document_set_name", r#""Alpha""#));

        value(state.create_project_session("tab-b"));
        value(state.engine_call("document_set_name", r#""Beta""#));

        assert_eq!(value(state.activate_project_session("tab-a")), true);
        assert_eq!(state.document_snapshot().name, "Alpha");
        assert_eq!(value(state.activate_project_session("tab-b")), true);
        assert_eq!(state.document_snapshot().name, "Beta");

        value(state.activate_project_session("tab-a"));
        value(state.drop_project_session("tab-b"));
        assert_eq!(value(state.activate_project_session("tab-b")), false);
    }

    #[test]
    fn export_precondition_runs_under_the_owning_workspace_lock() {
        let state = NativeEngineHost::new();
        value(state.bind_project_session("same-tab"));
        let expected = value(state.engine_call("project_export_model", ""));
        let request = serde_json::json!({"expected_model_json":expected});
        std::thread::scope(|threads| {
            let mut workspace = state.inner.lock().unwrap();
            let exporting = threads.spawn(|| state.export_stl(&request.to_string()));
            let exporting_step = threads.spawn(|| state.export_step(&request.to_string()));
            value(host::handle(
                &mut workspace.active_mut().manager,
                "document_set_name",
                r#""Replaced""#,
            ));
            drop(workspace);
            assert!(exporting
                .join()
                .unwrap()
                .unwrap_err()
                .contains("document changed"));
            assert!(exporting_step
                .join()
                .unwrap()
                .unwrap_err()
                .contains("document changed"));
        });
        assert!(state
            .export_3mf(&request.to_string())
            .unwrap_err()
            .contains("document changed"));
        assert_eq!(state.document_snapshot().name, "Replaced");
        assert_eq!(state.active_project_session_id(), "same-tab");
    }

    #[test]
    fn binding_recovered_bootstrap_session_preserves_the_solid_model() {
        let state = NativeEngineHost::new();
        value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
        value(state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-10.0,"y":-10.0},
                "p2":{"x":10.0,"y":10.0},
                "ctrl_held":false
            }"#,
        ));
        value(state.engine_call("end_sketch", ""));
        value(state.solid_extrude(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "operation":"new_body",
                "extent":{"type":"distance","distance":10.0},
                "taper_angle_deg":0.0,
                "flip":false,
                "target_body_ids":[]
            }"#,
        ));

        let (before_id, before_revision, before_scene, _, _, _, _, _, _, _) =
            state.viewport_snapshot();
        assert_eq!(before_id, BOOTSTRAP_SESSION_ID);
        assert_eq!(before_scene.bodies.len(), 1);
        assert!(!before_scene.bodies[0].faces.is_empty());

        value(state.bind_project_session("recovered-tab"));
        let (after_id, after_revision, after_scene, _, _, _, _, _, _, _) =
            state.viewport_snapshot();
        assert_eq!(after_id, "recovered-tab");
        assert_eq!(after_revision, before_revision);
        assert_eq!(after_scene.bodies.len(), before_scene.bodies.len());
        assert_eq!(
            after_scene.bodies[0].mesh.indices,
            before_scene.bodies[0].mesh.indices
        );
        assert_eq!(
            after_scene.bodies[0].mesh.positions,
            before_scene.bodies[0].mesh.positions
        );
    }
}
