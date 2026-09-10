use std::collections::HashMap;
use std::sync::Mutex;

use nbcad_core::{BodyAppearance, DocumentDto};
use nbcad_occt::{
    exact_interference_report, exact_pair_result, DrawingProjectionRequest, OcctKernel,
};
use nbcad_sketch::{
    approximate_pair_result, contact_violation_score, err_json, host, ok_json, BodyPoseDto,
    ContactSetDto, EvaluateMotionStudyRequestDto, InstanceBodyPoseDto, InterferenceCheckRequestDto,
    InterferencePairResultDto, InterferenceReportDto, MotionStudyEvaluationDto, MotionStudyId,
    MotionStudySampleDto, SampleMotionStudyRequestDto, SketchDto, SketchManager,
    SweptCollisionEventDto, SweptCollisionReportDto, SweptCollisionRequestDto,
};
use nbcad_solid::{
    BodyFeatureRequestDto, DatumPlaneDefinitionDto, DeleteFeatureRequest, EditBodyFeatureRequest,
    EditExtrudeRequest, EditHoleRequest, EditLoftRequest, EditRevolveRequest, EditRibRequest,
    EditSolidChamferRequest, EditSolidFilletRequest, EditSweepRequest, ExtrudeRequest, HoleRequest,
    LoftRequest, ProfileCatalogItemDto, RecomputePlanDto, ReorderFeatureRequest, RevolveRequest,
    RibRequest, SetRollbackRequest, SolidChamferRequest, SolidFilletRequest, SolidSceneDto,
    SolidUpdateDto, StepExportRequest, SweepRequest,
};
use serde::de::DeserializeOwned;

pub(crate) const BOOTSTRAP_SESSION_ID: &str = "__bootstrap__";
const MAX_PROJECT_SESSIONS: usize = 128;

struct NativeEngine {
    manager: SketchManager,
    kernel: OcctKernel,
    geometry_revision: u64,
}

impl NativeEngine {
    fn new() -> Result<Self, String> {
        Ok(Self {
            manager: SketchManager::new(),
            kernel: OcctKernel::new()
                .map_err(|error| format!("native OCCT kernel failed to initialize: {error}"))?,
            geometry_revision: 1,
        })
    }

    fn update(&self) -> SolidUpdateDto {
        SolidUpdateDto {
            document: self.manager.document_dto(),
            scene: self.manager.solid_scene(),
        }
    }
}

struct NativeWorkspace {
    active_session_id: String,
    sessions: HashMap<String, NativeEngine>,
}

impl NativeWorkspace {
    fn new() -> Self {
        let mut sessions = HashMap::new();
        sessions.insert(
            BOOTSTRAP_SESSION_ID.to_string(),
            NativeEngine::new().expect("native OCCT kernel failed to initialize"),
        );
        Self {
            active_session_id: BOOTSTRAP_SESSION_ID.to_string(),
            sessions,
        }
    }

    fn active(&self) -> &NativeEngine {
        self.sessions
            .get(&self.active_session_id)
            .expect("active project session missing")
    }

    fn active_mut(&mut self) -> &mut NativeEngine {
        self.sessions
            .get_mut(&self.active_session_id)
            .expect("active project session missing")
    }
}

/// Native application state: the shared Rust document/history manager plus
/// the stateful OCCT B-rep bridge. The whole pair is locked together so a
/// prepare → kernel replay → commit transaction cannot interleave.
pub struct AppState {
    inner: Mutex<NativeWorkspace>,
}

impl AppState {
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
    /// frontend's first tab. Repeated binding of the active tab is harmless.
    pub fn bind_project_session(&self, session_id: &str) -> String {
        if let Err(error) = validate_session_id(session_id) {
            return err_json(error);
        }
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        if workspace.active_session_id == session_id {
            return ok_json(());
        }
        if workspace.sessions.contains_key(session_id) {
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
            return err_json("too many resident project sessions");
        }
        let engine = match NativeEngine::new() {
            Ok(engine) => engine,
            Err(error) => return err_json(error),
        };
        let update = engine.update();
        workspace.sessions.insert(session_id.to_string(), engine);
        workspace.active_session_id = session_id.to_string();
        ok_json(update)
    }

    /// Activate a retained project. A missing value means the tab was evicted
    /// and should be recreated from its frontend-owned model snapshot.
    pub fn activate_project_session(&self, session_id: &str) -> String {
        if let Err(error) = validate_session_id(session_id) {
            return err_json(error);
        }
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        if !workspace.sessions.contains_key(session_id) {
            return ok_json(false);
        }
        workspace.active_session_id = session_id.to_string();
        ok_json(true)
    }

    /// Release an inactive tab's OCCT B-reps and tessellation. The frontend
    /// retains the parametric snapshot required to recreate it later.
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

    pub fn document_snapshot(&self) -> DocumentDto {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .document_dto()
    }

    /// One lock acquisition gives the native viewport a coherent model
    /// snapshot. The OCCT triangle buffers stay in Rust and never make a
    /// JSON/IPC round-trip through the webview.
    pub fn viewport_snapshot(
        &self,
    ) -> (
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
    ) {
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let inner = workspace.active();
        let assembly_solution = inner.manager.assembly_solution();
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
        if method == "drawing_export" {
            return self.drawing_export(payload);
        }
        if method == "drawing_projection" {
            return self.drawing_projection(payload);
        }
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        let inner = workspace.active_mut();
        let result = host::handle(&mut inner.manager, method, payload);
        if matches!(method, "datum_plane_create" | "datum_plane_edit") {
            inner.geometry_revision = inner.geometry_revision.wrapping_add(1);
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
        self.execute(|manager| {
            let raw = host::handle(manager, method, payload);
            let envelope: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
                nbcad_sketch::SessionError::Solid(format!("invalid engine response: {error}"))
            })?;
            if envelope.get("ok").and_then(|value| value.as_bool()) != Some(true) {
                let message = envelope
                    .get("error")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown noBS CAD engine error")
                    .to_string();
                return Err(nbcad_sketch::SessionError::Solid(message));
            }
            let value = envelope
                .get("value")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            serde_json::from_value(value).map_err(|error| {
                nbcad_sketch::SessionError::Solid(format!(
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
            manager.prepare_body_feature(request)
        })
    }

    pub fn solid_edit_body_feature(&self, payload: &str) -> String {
        self.with_request(payload, |manager, request: EditBodyFeatureRequest| {
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
        self.with_request(payload, |manager, model_json: String| {
            manager.prepare_load_project(model_json)
        })
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
            &inner.manager.solid_scene(),
            &solution.instance_body_poses,
            &request,
        ) {
            Ok(report) => ok_json(report),
            Err(error) => err_json(error),
        }
    }

    pub fn assembly_evaluate_motion_study(&self, payload: &str) -> String {
        let request: EvaluateMotionStudyRequestDto = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        let workspace = match self.inner.lock() {
            Ok(workspace) => workspace,
            Err(_) => return err_json("engine lock poisoned"),
        };
        let inner = workspace.active();
        let document = inner.manager.assembly_document();
        let candidate = match inner
            .manager
            .sample_motion_study(SampleMotionStudyRequestDto {
                study_id: request.study_id,
                time_seconds: request.time_seconds,
            }) {
            Ok(sample) => sample,
            Err(error) => return err_json(error.to_string()),
        };
        let mut final_sample = candidate;
        let mut stopped_by_contact = None;
        let mut stop_time_seconds = None;
        let enabled_contacts = document
            .contact_sets
            .iter()
            .filter(|contact| contact.enabled)
            .collect::<Vec<_>>();
        if request.enforce_contacts && enabled_contacts.iter().any(|contact| contact.stop_motion) {
            let start_time = request.previous_time_seconds.unwrap_or(0.0);
            let start = inner
                .manager
                .sample_motion_study(SampleMotionStudyRequestDto {
                    study_id: request.study_id,
                    time_seconds: start_time,
                });
            if let Ok(start) = start {
                for contact in enabled_contacts
                    .iter()
                    .copied()
                    .filter(|contact| contact.stop_motion)
                {
                    let start_violation = match gated_exact_contact_violation(
                        &inner.kernel,
                        &inner.manager.solid_scene(),
                        &start,
                        contact,
                    ) {
                        Ok(violation) => violation,
                        Err(error) => return err_json(error),
                    };
                    let end_violation = match gated_exact_contact_violation(
                        &inner.kernel,
                        &inner.manager.solid_scene(),
                        &final_sample,
                        contact,
                    ) {
                        Ok(violation) => violation,
                        Err(error) => return err_json(error),
                    };
                    if start_violation > 1.0e-7 && end_violation >= start_violation {
                        final_sample = start;
                        stopped_by_contact = Some(contact.id);
                        stop_time_seconds = Some(final_sample.time_seconds);
                        break;
                    }
                    if start_violation <= 1.0e-7 {
                        let crossing = match first_exact_contact_crossing(
                            &inner.manager,
                            &inner.kernel,
                            request.study_id,
                            &start,
                            &final_sample,
                            contact,
                            end_violation,
                        ) {
                            Ok(crossing) => crossing,
                            Err(error) => return err_json(error),
                        };
                        if let Some(sample) = crossing {
                            final_sample = sample;
                            let stop_time = final_sample.time_seconds;
                            stopped_by_contact = Some(contact.id);
                            stop_time_seconds = Some(stop_time);
                            break;
                        }
                    }
                }
            }
        }
        // Playback reports only configured contact pairs. Full all-body OCCT
        // interference remains an explicit Inspect command and is never paid
        // on every animation frame.
        let mut contact_pairs = Vec::with_capacity(enabled_contacts.len());
        let mut contacts_exact = true;
        for contact in enabled_contacts {
            match gated_contact_result(
                &inner.kernel,
                &inner.manager.solid_scene(),
                &final_sample,
                contact,
            ) {
                Ok((pair, exact)) => {
                    contacts_exact &= exact;
                    contact_pairs.push(pair);
                }
                Err(error) => return err_json(error),
            }
        }
        let contacts = InterferenceReportDto {
            exact: contacts_exact,
            pairs: contact_pairs,
        };
        ok_json(MotionStudyEvaluationDto {
            sample: final_sample,
            contacts,
            stopped_by_contact,
            stop_time_seconds,
        })
    }

    pub fn assembly_swept_collision_check(&self, payload: &str) -> String {
        let request: SweptCollisionRequestDto = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        if !request.sample_rate_hz.is_finite() || !(1.0..=240.0).contains(&request.sample_rate_hz) {
            return err_json("swept collision sample rate must be between 1 and 240 Hz");
        }
        if !request.clearance_threshold_mm.is_finite() || request.clearance_threshold_mm < 0.0 {
            return err_json("swept collision clearance must be finite and non-negative");
        }
        let workspace = match self.inner.lock() {
            Ok(workspace) => workspace,
            Err(_) => return err_json("engine lock poisoned"),
        };
        let inner = workspace.active();
        let document = inner.manager.assembly_document();
        let study = match document
            .motion_studies
            .iter()
            .find(|study| study.id == request.study_id)
        {
            Some(study) => study,
            None => {
                return err_json(format!(
                    "motion study {} does not exist",
                    request.study_id.0
                ))
            }
        };
        let count = (study.duration_seconds * request.sample_rate_hz).ceil() as u32 + 1;
        if count > 100_001 {
            return err_json("swept collision study exceeds 100,001 samples");
        }
        let mut events = HashMap::<(u64, u64, u64, u64), SweptCollisionEventDto>::new();
        for index in 0..count {
            let time = ((index as f64) / request.sample_rate_hz).min(study.duration_seconds);
            let sample = match inner
                .manager
                .sample_motion_study(SampleMotionStudyRequestDto {
                    study_id: request.study_id,
                    time_seconds: time,
                }) {
                Ok(sample) => sample,
                Err(error) => return err_json(error.to_string()),
            };
            let report = match exact_interference_report(
                &inner.kernel,
                &inner.manager.solid_scene(),
                &sample.solution.instance_body_poses,
                &InterferenceCheckRequestDto {
                    occurrence_ids: Vec::new(),
                    clearance_threshold_mm: request.clearance_threshold_mm,
                },
            ) {
                Ok(report) => report,
                Err(error) => return err_json(error),
            };
            for pair in report
                .pairs
                .into_iter()
                .filter(|pair| pair.interfering || pair.below_clearance)
            {
                let key = (
                    pair.occurrence_a.0,
                    pair.body_a.0,
                    pair.occurrence_b.0,
                    pair.body_b.0,
                );
                events
                    .entry(key)
                    .and_modify(|event| {
                        event.last_time_seconds = time;
                        event.minimum_clearance_mm =
                            event.minimum_clearance_mm.min(pair.minimum_clearance_mm);
                        event.maximum_overlap_volume_mm3 = event
                            .maximum_overlap_volume_mm3
                            .max(pair.overlap_volume_mm3);
                    })
                    .or_insert(SweptCollisionEventDto {
                        occurrence_a: pair.occurrence_a,
                        body_a: pair.body_a,
                        occurrence_b: pair.occurrence_b,
                        body_b: pair.body_b,
                        first_time_seconds: time,
                        last_time_seconds: time,
                        minimum_clearance_mm: pair.minimum_clearance_mm,
                        maximum_overlap_volume_mm3: pair.overlap_volume_mm3,
                    });
            }
            if request.stop_at_first && !events.is_empty() {
                let mut result = events.into_values().collect::<Vec<_>>();
                result.sort_by(|a, b| a.first_time_seconds.total_cmp(&b.first_time_seconds));
                return ok_json(SweptCollisionReportDto {
                    exact: true,
                    sample_count: index + 1,
                    events: result,
                });
            }
        }
        let mut result = events.into_values().collect::<Vec<_>>();
        result.sort_by(|a, b| a.first_time_seconds.total_cmp(&b.first_time_seconds));
        ok_json(SweptCollisionReportDto {
            exact: true,
            sample_count: count,
            events: result,
        })
    }

    pub fn export_step(&self, payload: &str) -> Result<Vec<u8>, String> {
        let request: StepExportRequest = serde_json::from_str(payload)
            .map_err(|error| format!("bad request payload: {error}"))?;
        let workspace = self
            .inner
            .lock()
            .map_err(|_| "engine lock poisoned".to_string())?;
        let inner = workspace.active();
        if !inner.manager.solid_scene().errors.is_empty() {
            return Err("Resolve timeline errors before exporting STEP.".to_string());
        }
        inner
            .kernel
            .export_step(&request)
            .map_err(|error| error.to_string())
    }

    pub fn drawing_export(&self, payload: &str) -> String {
        let request: nbcad_occt::drawing_export::DrawingExportRequest =
            match serde_json::from_str(payload) {
                Ok(request) => request,
                Err(error) => return err_json(format!("bad request payload: {error}")),
            };
        let workspace = match self.inner.lock() {
            Ok(workspace) => workspace,
            Err(_) => return err_json("engine lock poisoned"),
        };
        let inner = workspace.active();
        let scene = inner.manager.solid_scene();
        let content = nbcad_occt::drawing_export::export_sheet(
            &inner.manager.drawing_document(),
            &scene,
            &inner.manager.assembly_document(),
            &request,
            |r| {
                        nbcad_occt::project_drawing(&inner.kernel, &scene, &inner.manager.assembly_document(), r).map_err(|e|e.to_string())
            },
        );
        match content {
            Ok(content) => ok_json(
                serde_json::json!({"format":request.format,"encoding":"utf8","content":content,"sheet_id":request.sheet_id}),
            ),
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
        let scene = inner.manager.solid_scene();
        if !scene.errors.is_empty() {
            return err_json("Resolve timeline errors before generating a drawing view.");
        }
        match nbcad_occt::project_drawing(
            &inner.kernel,
            &scene,
            &inner.manager.assembly_document(),
            &request,
        ) {
            Ok(projection) => ok_json(projection),
            Err(error) => err_json(error.to_string()),
        }
    }

    pub fn export_stl(&self, payload: &str) -> Result<Vec<u8>, String> {
        let request: nbcad_export::MeshExportRequest = serde_json::from_str(payload)
            .map_err(|error| format!("bad request payload: {error}"))?;
        let workspace = self
            .inner
            .lock()
            .map_err(|_| "engine lock poisoned".to_string())?;
        let inner = workspace.active();
        if !inner.manager.solid_scene().errors.is_empty() {
            return Err("Resolve timeline errors before exporting STL.".to_string());
        }
        let scene = inner.manager.solid_scene();
        let mut meshes = inner
            .kernel
            .tessellate_bodies(&request)
            .map_err(|error| error.to_string())?;
        for mesh in &mut meshes {
            if let Some(body) = scene.bodies.iter().find(|body| body.id == mesh.body_id) {
                mesh.name = body.name.clone();
            }
        }
        let solution = inner.manager.assembly_solution();
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
        nbcad_export::write_stl(&meshes).map_err(|error| error.to_string())
    }

    pub fn export_3mf(&self, payload: &str) -> Result<Vec<u8>, String> {
        let request: nbcad_export::MeshExportRequest = serde_json::from_str(payload)
            .map_err(|error| format!("bad request payload: {error}"))?;
        let workspace = self
            .inner
            .lock()
            .map_err(|_| "engine lock poisoned".to_string())?;
        let inner = workspace.active();
        if !inner.manager.solid_scene().errors.is_empty() {
            return Err("Resolve timeline errors before exporting 3MF.".to_string());
        }
        let scene = inner.manager.solid_scene();
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
        let solution = inner.manager.assembly_solution();
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
        nbcad_export::ExportFacade::export_3mf(&meshes, &appearances, &request)
            .map_err(|error| error.to_string())
    }

    fn with_request<T: DeserializeOwned>(
        &self,
        payload: &str,
        prepare: impl FnOnce(
            &mut SketchManager,
            T,
        ) -> Result<RecomputePlanDto, nbcad_sketch::SessionError>,
    ) -> String {
        let request = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(error) => return err_json(format!("bad request payload: {error}")),
        };
        self.execute(|manager| prepare(manager, request))
    }

    fn execute(
        &self,
        prepare: impl FnOnce(&mut SketchManager) -> Result<RecomputePlanDto, nbcad_sketch::SessionError>,
    ) -> String {
        let mut workspace = self.inner.lock().expect("engine lock poisoned");
        let inner = workspace.active_mut();
        let plan = match prepare(&mut inner.manager) {
            Ok(plan) => plan,
            Err(error) => return err_json(error.to_string()),
        };
        let transaction_id = plan.transaction_id;
        let kernel_scene = match inner.kernel.recompute(&plan) {
            Ok(scene) => scene,
            Err(error) => {
                inner.manager.cancel_solid_recompute(transaction_id);
                return err_json(error.to_string());
            }
        };
        match inner
            .manager
            .commit_solid(nbcad_solid::CommitKernelRequest {
                transaction_id,
                scene: kernel_scene,
            }) {
            Ok(update) => {
                inner.geometry_revision = inner.geometry_revision.wrapping_add(1);
                ok_json(update)
            }
            Err(error) => err_json(error.to_string()),
        }
    }
}

fn gated_exact_contact_violation(
    kernel: &OcctKernel,
    scene: &SolidSceneDto,
    sample: &MotionStudySampleDto,
    contact: &ContactSetDto,
) -> Result<f64, String> {
    let (result, _) = gated_contact_result(kernel, scene, sample, contact)?;
    Ok(contact_violation_score(&result, contact.clearance_mm))
}

fn gated_contact_result(
    kernel: &OcctKernel,
    scene: &SolidSceneDto,
    sample: &MotionStudySampleDto,
    contact: &ContactSetDto,
) -> Result<(InterferencePairResultDto, bool), String> {
    let a = sample
        .solution
        .instance_body_poses
        .iter()
        .find(|pose| pose.occurrence_id == contact.occurrence_a && pose.body_id == contact.body_a)
        .ok_or_else(|| format!("contact '{}' first placed body is missing", contact.name))?;
    let b = sample
        .solution
        .instance_body_poses
        .iter()
        .find(|pose| pose.occurrence_id == contact.occurrence_b && pose.body_id == contact.body_b)
        .ok_or_else(|| format!("contact '{}' second placed body is missing", contact.name))?;
    let broad = approximate_pair_result(scene, a, b, contact.clearance_mm)?;
    if contact_violation_score(&broad, contact.clearance_mm) <= 1.0e-7 {
        return Ok((broad, false));
    }
    let exact = exact_pair_result(kernel, a, b, contact.clearance_mm)?;
    Ok((exact, true))
}

/// Search the full frame interval rather than only its endpoints. Cheap mesh
/// bounds gate all OCCT work; exact B-rep checks happen only while the chosen
/// contact pair can actually touch. Eight ordered probes catch short
/// enter/exit events across a normal 30 Hz playback frame before bisection.
fn first_exact_contact_crossing(
    manager: &SketchManager,
    kernel: &OcctKernel,
    study_id: MotionStudyId,
    start: &MotionStudySampleDto,
    end: &MotionStudySampleDto,
    contact: &ContactSetDto,
    end_violation: f64,
) -> Result<Option<MotionStudySampleDto>, String> {
    const PROBE_STEPS: usize = 8;
    const BISECTION_STEPS: usize = 18;
    let scene = manager.solid_scene();
    let mut safe_time = start.time_seconds;
    for step in 1..=PROBE_STEPS {
        let fraction = step as f64 / PROBE_STEPS as f64;
        let time = start.time_seconds + (end.time_seconds - start.time_seconds) * fraction;
        let sample = if step == PROBE_STEPS {
            end.clone()
        } else {
            manager
                .sample_motion_study(SampleMotionStudyRequestDto {
                    study_id,
                    time_seconds: time,
                })
                .map_err(|error| error.to_string())?
        };
        let violation = if step == PROBE_STEPS {
            end_violation
        } else {
            gated_exact_contact_violation(kernel, &scene, &sample, contact)?
        };
        if violation <= 1.0e-7 {
            safe_time = sample.time_seconds;
            continue;
        }

        let mut safe = safe_time;
        let mut blocked = sample.time_seconds;
        for _ in 0..BISECTION_STEPS {
            let middle = (safe + blocked) * 0.5;
            let candidate = manager
                .sample_motion_study(SampleMotionStudyRequestDto {
                    study_id,
                    time_seconds: middle,
                })
                .map_err(|error| error.to_string())?;
            if gated_exact_contact_violation(kernel, &scene, &candidate, contact)? > 1.0e-7 {
                blocked = middle;
            } else {
                safe = middle;
            }
        }
        return manager
            .sample_motion_study(SampleMotionStudyRequestDto {
                study_id,
                time_seconds: blocked,
            })
            .map(Some)
            .map_err(|error| error.to_string());
    }
    Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn value(json: String) -> serde_json::Value {
        let envelope: serde_json::Value = serde_json::from_str(&json).expect("valid envelope");
        assert_eq!(envelope["ok"], true, "engine error: {envelope}");
        envelope["value"].clone()
    }

    #[test]
    fn project_sessions_retain_and_release_independent_documents() {
        let state = AppState::new();
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
    fn binding_recovered_bootstrap_session_preserves_the_solid_model() {
        let state = AppState::new();
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
