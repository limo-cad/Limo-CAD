//! Sketch manager: owns the document plus the sketch-session lifecycle
//! (`begin_sketch` / `end_sketch`) and routes drawing ops to the active
//! session. This is the object both engine hosts (native, WASM) hold.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

mod component_edit;
mod component_removal;
mod rename;

use serde::Serialize;

use limo_cad_assembly::{
    approximate_interference_report, approximate_pair_result, contact_violation_score,
    ApplyJointMotionsRequestDto, AssemblyDocumentDto, AssemblyPositionDto, AssemblyPositionId,
    AssemblySolutionDto, ComponentDefinitionDto, ComponentOccurrenceDto, ContactSetDto,
    ContactSetId, CreateAssemblyPositionRequestDto, CreateComponentRequestDto,
    CreateContactSetRequestDto, CreateGearRelationRequestDto, CreateJointRequestDto,
    CreateMotionStudyRequestDto, CreateOccurrenceRequestDto, DuplicateOccurrenceRequestDto,
    EvaluateMotionStudyRequestDto, GearRelationDto, InterferenceCheckRequestDto,
    InterferenceReportDto, JointDefinitionDto, JointId, MechanismDragRequestDto,
    MechanismPreviewDto, MotionPathRequestDto, MotionStudyDto, MotionStudyEvaluationDto,
    MotionStudyId, MotionStudySampleDto, RemoveOccurrenceRequestDto, SampleMotionStudyRequestDto,
    SetJointCoordinatesRequestDto, SetJointEnabledRequestDto, SetJointMotionRequestDto,
    SetOccurrenceGroundedRequestDto, SetOccurrencePoseRequestDto, SweptCollisionEventDto,
    SweptCollisionReportDto, SweptCollisionRequestDto, UpdateComponentRequestDto,
    UpdateJointRequestDto, UpdateOccurrenceRequestDto,
};
use limo_cad_cam::{
    analyze_nbpost, plan_setup, post_event_stream, post_setup, simulate_gcode, simulate_setup,
    CamAdaptiveGeometryDto, CamChainSource, CamDocumentDto, CamGcodeSimulationRequestDto,
    CamHeightExpressionDto, CamHeightReferenceDto, CamHoleDto, CamOperationDto,
    CamOperationHeightExpressionsDto, CamPostRequestDto, CamPostResultDto, CamProgramDto,
    CamResolvedStockDto, CamSetupDto, CamSimulationRequestDto, CamSimulationResultDto,
    CamSimulationTargetDto, CamStockMeshDto, CamToolpathGenerationDto, CamToolpathStateDto,
    CamToolpathStatusDto, NbPostAnalysisDto, NbPostAnalysisRequestDto, PostEventStreamDto,
};
use limo_cad_core::{
    BodyAppearance, BodyId, BrowserNodeKind, Document, DocumentDto, EdgeId, FaceId, Feature,
    FeatureId, FeatureKind, FeatureStatus, PlaneBasis, PlaneRef, DEFAULT_MATERIAL_NAME,
};
use limo_cad_solid::{
    canonicalize_profile_curves, extract_bounded_faces, BodyFeatureDefinitionDto,
    BodyFeatureRequestDto, CommitKernelRequest, DatumPlaneDefinitionDto, DatumPlaneRequest,
    DatumPlaneSourceDto, DatumPlaneUpdateDto, DeleteFeatureRequest, EditBodyFeatureRequest,
    EditDatumPlaneRequest, EditExtrudeRequest, EditHoleRequest, EditLoftRequest,
    EditRevolveRequest, EditRibRequest, EditSolidChamferRequest, EditSolidFilletRequest,
    EditSweepRequest, ExtrudeDefinitionDto, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest,
    HoleDefinitionDto, HoleRequest, LoftDefinitionDto, LoftRequest, Point2Dto, Point3Dto,
    ProfileCatalogItemDto, ProfileCurveDto, ProfileLoopDto, RecomputePlanDto,
    ReorderFeatureRequest, RevolveDefinitionDto, RevolveRequest, RibDefinitionDto, RibExtent,
    RibRequest, Segment2, SetRollbackRequest, SketchLineDto, SketchPathCurveDto,
    SketchPointKindDto, SketchReferencePointDto, SolidChamferDefinitionDto, SolidChamferRequest,
    SolidDocument, SolidFilletDefinitionDto, SolidFilletRequest, SolidSceneDto, SolidUpdateDto,
    SweepDefinitionDto, SweepRequest,
};

use crate::constraint::{Constraint, ConstraintId};
use crate::drawing::DrawingDocumentDto;
use crate::dto::{
    AddConstraintResult, AddLineResult, Arc3PointRequest, ArcCenterRequest, BeginSketchRequest,
    BreakRequest, ChamferRequest, CircleRequest, CircularPatternRequest, ConstraintBatchRequest,
    DeleteEntityResult, DimensionRequest, EditDimensionRequest, EndSketchResult,
    EvalExpressionRequest, EvalExpressionResult, ExtendRequest, FaceSketchOrigin, FilletPreviewDto,
    FilletRequest, LockedCircleRequest, LockedRectangleRequest, LockedSegmentRequest,
    MidpointLineRequest, MirrorRequest, MoveCopyRequest, MoveDimensionRequest, MovePointRequest,
    MovePointResult, NamedViewConfigurationDto, NamedViewsDto, OffsetPreviewDto, OffsetRequest,
    PointRequest, PolygonRequest, PreviewDto, ProjectVisibilityDto, ProjectedEdgeDto,
    RecallNamedViewDto, RectangleRequest, RectangularPatternRequest, ScaleRequest, SegmentRequest,
    SetDimensionModeRequest, SetDimensionStyleRequest, SetGridSnapRequest, SetGridStepRequest,
    SketchDto, SlotRequest, SplineRequest, ToggleFixBatchRequest, ToolResult, TrimPreviewDto,
    TrimRequest, UndoResult,
};
use crate::entity::EntityId;
use crate::project::{
    decode_project, ProjectCountersV2, ProjectDocumentV2, ProjectModelV9, ProjectPreferencesV2,
    PROJECT_FORMAT, PROJECT_SCHEMA_VERSION,
};
use crate::session::{
    SessionError, SketchSession, GRID_STEP_MM, MAX_GRID_STEP_MM, MIN_GRID_STEP_MM,
};

mod print_heights;
mod print_intent;
mod print_modifiers;
mod retention;
pub use retention::RetainedSketchSessions;

/// A sketch that has been finished and is kept in the document. The full
/// session is retained (M1d): it renders muted in 3D and re-enters editing
/// via `edit_sketch` with entities, constraints, dimensions, and undo
/// intact.
#[derive(Debug)]
pub struct FinishedSketch {
    session: SketchSession,
    feature_id: FeatureId,
}

/// Document + sketch-session state shared by both hosts (D8).
#[derive(Debug)]
pub struct SketchManager {
    document: Document,
    active: Option<SketchSession>,
    active_feature_id: Option<FeatureId>,
    finished: Vec<FinishedSketch>,
    solids: SolidDocument,
    datum_planes: Vec<DatumPlaneDefinitionDto>,
    next_datum_id: u64,
    sketch_count: u32,
    extrude_count: u32,
    revolve_count: u32,
    sweep_count: u32,
    loft_count: u32,
    rib_count: u32,
    fillet_count: u32,
    chamfer_count: u32,
    hole_count: u32,
    /// Grid-snap preference applied to new sessions (Sketch Palette "Snap").
    grid_snap: bool,
    /// View-dependent grid spacing supplied by the viewport. This is runtime
    /// state rather than project state: reopening at a different zoom must
    /// choose the spacing appropriate for that view.
    grid_step: f64,
    /// Per-body color/material for viewport and manufacturing export.
    body_appearances: Vec<BodyAppearance>,
    /// Persistent technical-drawing sheets and view definitions.
    drawings: DrawingDocumentDto,
    /// Persistent assembly/joint intent. Kinematic display poses are derived
    /// by the assembly solver at runtime and never baked into solid history.
    assembly: AssemblyDocumentDto,
    /// Solving a large occurrence graph is deterministic but not free. The
    /// authoritative result is retained until assembly intent or source
    /// geometry changes; render snapshots and UI reads then share one solve.
    assembly_solution_cache: RefCell<Option<AssemblySolutionDto>>,
    /// Persistent Browser visibility expressed with stable model identities.
    project_visibility: ProjectVisibilityDto,
    /// Display layouts leave solid definitions intact.
    named_views: Vec<NamedViewConfigurationDto>,
    active_named_view: Option<String>,
    print_intent: limo_cad_core::PrintIntentDocumentDto,
    /// Persistent 3-axis manufacturing setups, tools, and operation intent.
    cam: CamDocumentDto,
    /// Candidate manager held until its OCCT replay commits successfully.
    /// Keeping the current manager alive makes Open transactional.
    pending_project: Option<PendingProject>,
    /// Armed only for an explicit history deletion. Rollback and ordinary
    /// recompute can hide a body temporarily and must retain assembly joints.
    pending_joint_body_deletion: Option<(u64, BTreeSet<BodyId>)>,
}

#[derive(Debug)]
struct PendingProject {
    transaction_id: u64,
    manager: Box<SketchManager>,
}

/// Bump this whenever planner semantics change in a way that should force
/// existing operations through explicit regeneration before NC posting.
const CAM_TOOLPATH_PLANNER_REVISION: u32 = 25;

#[cfg(test)]
#[path = "cam_verification_tests.rs"]
mod cam_verification_tests;

#[cfg(test)]
#[path = "cam_order_tests.rs"]
mod cam_order_tests;

#[cfg(test)]
#[path = "cam_tool_compatibility_tests.rs"]
mod cam_tool_compatibility_tests;

#[cfg(test)]
#[path = "cam_fingerprint_tests.rs"]
mod cam_fingerprint_tests;

struct CamSetupDependencyFingerprints {
    model: String,
    setup: String,
    upstream: String,
}

fn stable_cam_fingerprint<T: Serialize + ?Sized>(value: &T) -> Result<String, SessionError> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        SessionError::Solid(format!("could not fingerprint CAM inputs: {error}"))
    })?;

    let mut hash = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    for byte in bytes {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    Ok(format!("{hash:032x}"))
}

impl SketchManager {
    pub fn new() -> Self {
        Self {
            document: Document::new("Untitled"),
            active: None,
            active_feature_id: None,
            finished: Vec::new(),
            solids: SolidDocument::new(),
            datum_planes: Vec::new(),
            next_datum_id: 1,
            sketch_count: 0,
            extrude_count: 0,
            revolve_count: 0,
            sweep_count: 0,
            loft_count: 0,
            rib_count: 0,
            fillet_count: 0,
            chamfer_count: 0,
            hole_count: 0,
            grid_snap: true,
            grid_step: GRID_STEP_MM,
            body_appearances: Vec::new(),
            drawings: DrawingDocumentDto::default(),
            assembly: AssemblyDocumentDto::default(),
            assembly_solution_cache: RefCell::new(None),
            project_visibility: ProjectVisibilityDto::default(),
            named_views: Vec::new(),
            active_named_view: None,
            print_intent: limo_cad_core::PrintIntentDocumentDto::default(),
            cam: CamDocumentDto::default(),
            pending_project: None,
            pending_joint_body_deletion: None,
        }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn document_dto(&self) -> DocumentDto {
        DocumentDto::from(&self.document)
    }

    /// Script playback may start only in a document with no authored work.
    /// Shared by the desktop lesson controls and the existing script runner.
    pub fn is_blank_for_script(&self) -> bool {
        self.active.is_none()
            && self.document.features().features.is_empty()
            && self.solids.scene().bodies.is_empty()
            && self.drawings.sheets.is_empty()
            && self.assembly == AssemblyDocumentDto::default()
            && self.cam == CamDocumentDto::default()
            && self.named_views.is_empty()
            && self.print_intent == limo_cad_core::PrintIntentDocumentDto::default()
    }

    pub fn set_document_name(&mut self, name: String) -> Result<DocumentDto, SessionError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(SessionError::Solid(
                "document name cannot be empty".to_string(),
            ));
        }
        self.document.set_name(name);
        Ok(self.document_dto())
    }

    /// Rename an operation and its persisted source references without recomputing geometry.
    pub fn rename_solid_feature(
        &mut self,
        feature_id: FeatureId,
        name: String,
    ) -> Result<DocumentDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before renaming a feature".into(),
            ));
        }
        if self.pending_project.is_some() {
            return Err(SessionError::Solid(
                "features cannot be renamed during project replacement".into(),
            ));
        }
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 256 || name.chars().any(char::is_control) {
            return Err(SessionError::Solid(
                "feature name must contain 1 to 256 characters without control characters".into(),
            ));
        }
        let index = self
            .document
            .features()
            .features
            .iter()
            .position(|feature| feature.id == feature_id)
            .ok_or_else(|| SessionError::Solid("the history feature no longer exists".into()))?;
        match self.document.features().features[index].kind {
            FeatureKind::Sketch => self.rename_sketch(feature_id, name)?,
            FeatureKind::ConstructionPlane => {
                return Err(SessionError::Solid(
                    "name datum planes when creating them".into(),
                ));
            }
            _ => self
                .solids
                .rename_feature(feature_id, name)
                .map_err(|error| SessionError::Solid(error.to_string()))?,
        }
        let feature = &mut self.document.features_mut().features[index];
        feature.name.clear();
        feature.name.push_str(name);
        Ok(self.document_dto())
    }

    /// Serialize the authoritative parametric model. Tessellation and B-reps
    /// are intentionally excluded and are regenerated on Open.
    pub fn export_project_model(&self) -> Result<String, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before saving the project".to_string(),
            ));
        }
        let model = ProjectModelV9 {
            format: PROJECT_FORMAT.to_string(),
            schema_version: PROJECT_SCHEMA_VERSION,
            document: ProjectDocumentV2 {
                name: self.document.name().to_string(),
                settings: self.document.settings().clone(),
                history: self.document.features().clone(),
            },
            sketches: self
                .finished
                .iter()
                .map(|finished| finished.session.project_state(finished.feature_id))
                .collect(),
            extrudes: self.solids.definitions().to_vec(),
            revolves: self.solids.revolve_definitions().to_vec(),
            sweeps: self.solids.sweep_definitions().to_vec(),
            lofts: self.solids.loft_definitions().to_vec(),
            ribs: self.solids.rib_definitions().to_vec(),
            fillets: self.solids.fillet_definitions().to_vec(),
            chamfers: self.solids.chamfer_definitions().to_vec(),
            holes: self.solids.hole_definitions().to_vec(),
            datum_planes: self.datum_planes.clone(),
            body_features: self.solids.body_feature_definitions().to_vec(),
            body_appearances: self.scrubbed_body_appearances(),
            drawings: self.drawings.clone(),
            assembly: self.assembly.clone(),
            visibility: self.scrubbed_project_visibility(),
            views: self.scrubbed_named_views(),
            print_intent: self.print_intent.clone(),
            cam: self.cam.clone(),
            counters: ProjectCountersV2 {
                sketch: self.sketch_count,
                extrude: self.extrude_count,
                revolve: self.revolve_count,
                sweep: self.sweep_count,
                loft: self.loft_count,
                rib: self.rib_count,
                fillet: self.fillet_count,
                chamfer: self.chamfer_count,
                hole: self.hole_count,
            },
            preferences: ProjectPreferencesV2 {
                grid_snap: self.grid_snap,
            },
        };
        serde_json::to_string_pretty(&model)
            .map_err(|error| SessionError::Solid(format!("could not serialize project: {error}")))
    }

    /// Prepare a fresh untitled project through the same transactional replay
    /// path as Open. The current project remains intact until the host kernel
    /// accepts and commits the empty scene.
    pub fn prepare_new_project(&mut self) -> Result<RecomputePlanDto, SessionError> {
        let model_json = SketchManager::new().export_project_model()?;
        self.prepare_load_project(model_json)
    }

    /// Parse and validate `model.json`, construct a candidate document, and
    /// return its full-replay plan. The current document is only replaced
    /// after the kernel scene commits.
    pub fn prepare_load_project(
        &mut self,
        model_json: String,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.prepare_load_project_ref(&model_json)
    }

    /// Prepare from retained archive text without copying it. Decoding owns
    /// the candidate data; the input is never stored or changed, so a cold
    /// document can keep its recovery snapshot intact if reconstruction fails.
    pub fn prepare_load_project_ref(
        &mut self,
        model_json: &str,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.pending_project.is_some() {
            return Err(SessionError::Solid(
                "a project open is already pending".to_string(),
            ));
        }
        let mut model = decode_project(model_json).map_err(SessionError::Solid)?;
        let mut document = Document::new(model.document.name);
        document.restore_history(model.document.settings, model.document.history);

        let feature_order = document
            .features()
            .features
            .iter()
            .enumerate()
            .map(|(index, feature)| (feature.id, index))
            .collect::<std::collections::HashMap<_, _>>();
        let mut datum_planes = model.datum_planes;
        datum_planes.sort_by_key(|plane| feature_order[&plane.feature_id]);
        for plane in &datum_planes {
            document.add_construction_plane_node(plane.datum_id.0, &plane.name);
        }
        let mut saved_sketches = model.sketches;
        saved_sketches.sort_by_key(|sketch| feature_order[&sketch.feature_id]);
        let mut finished = Vec::with_capacity(saved_sketches.len());
        for saved in saved_sketches {
            let feature_id = saved.feature_id;
            let session = SketchSession::from_project_state(saved)?;
            document.add_browser_child(
                BrowserNodeKind::SketchesFolder,
                BrowserNodeKind::Sketch,
                session.name(),
            );
            finished.push(FinishedSketch {
                session,
                feature_id,
            });
        }

        let mut solids =
            SolidDocument::restore_feature_definitions(limo_cad_solid::SolidFeatureDefinitions {
                extrudes: model.extrudes,
                revolves: model.revolves,
                sweeps: model.sweeps,
                lofts: model.lofts,
                ribs: model.ribs,
                fillets: model.fillets,
                chamfers: model.chamfers,
                holes: model.holes,
                body_features: model.body_features,
            })
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        if let Some(body_id) = print_intent::print_intent_body_floor(&model.print_intent) {
            solids
                .reserve_body_ids_through(body_id)
                .map_err(|error| SessionError::Solid(error.to_string()))?;
        }
        model.assembly.component_structure.next_occurrence_id =
            model.assembly.component_structure.next_occurrence_id.max(
                print_intent::print_intent_occurrence_floor(&model.print_intent),
            );
        let mut candidate = SketchManager {
            document,
            active: None,
            active_feature_id: None,
            finished,
            solids,
            next_datum_id: datum_planes
                .iter()
                .map(|plane| plane.datum_id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1)
                .max(1),
            datum_planes,
            sketch_count: model.counters.sketch,
            extrude_count: model.counters.extrude,
            revolve_count: model.counters.revolve,
            sweep_count: model.counters.sweep,
            loft_count: model.counters.loft,
            rib_count: model.counters.rib,
            fillet_count: model.counters.fillet,
            chamfer_count: model.counters.chamfer,
            hole_count: model.counters.hole,
            grid_snap: model.preferences.grid_snap,
            grid_step: GRID_STEP_MM,
            body_appearances: model.body_appearances,
            drawings: model.drawings,
            assembly: model.assembly,
            assembly_solution_cache: RefCell::new(None),
            project_visibility: model.visibility,
            named_views: model.views,
            active_named_view: None,
            print_intent: model.print_intent,
            cam: model.cam,
            pending_project: None,
            pending_joint_body_deletion: None,
        };
        candidate.sketch_count = candidate
            .sketch_count
            .max(max_numbered_name(&candidate.finished, "Sketch"));
        candidate.extrude_count = candidate
            .extrude_count
            .max(max_feature_number(&candidate.document, "Extrude"));
        candidate.revolve_count = candidate
            .revolve_count
            .max(max_feature_number(&candidate.document, "Revolve"));
        candidate.sweep_count = candidate
            .sweep_count
            .max(max_feature_number(&candidate.document, "Sweep"));
        candidate.loft_count = candidate
            .loft_count
            .max(max_feature_number(&candidate.document, "Loft"));
        candidate.rib_count = candidate
            .rib_count
            .max(max_feature_number(&candidate.document, "Rib"));
        candidate.fillet_count = candidate
            .fillet_count
            .max(max_feature_number(&candidate.document, "Fillet"));
        candidate.chamfer_count = candidate
            .chamfer_count
            .max(max_feature_number(&candidate.document, "Chamfer"));
        candidate.hole_count = candidate
            .hole_count
            .max(max_feature_number(&candidate.document, "Hole"));
        let feature_order = candidate
            .document
            .features()
            .features
            .iter()
            .map(|feature| feature.id)
            .collect::<Vec<_>>();
        candidate
            .solids
            .set_feature_order(&feature_order)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        let active = candidate.active_feature_ids();
        let catalog = candidate.profile_catalog();
        let plan = candidate
            .solids
            .prepare_recompute_resilient(&catalog, &active)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        candidate.sync_named_view_browser();
        self.pending_project = Some(PendingProject {
            transaction_id: plan.transaction_id,
            manager: Box::new(candidate),
        });
        Ok(plan)
    }

    /// Start a sketch on `plane`: names it "Sketch1", "Sketch2", … and
    /// registers it in the browser tree under Sketches.
    pub fn begin_sketch(&mut self, plane: PlaneRef) -> Result<SketchDto, SessionError> {
        self.begin_sketch_with_options(BeginSketchRequest {
            name: None,
            plane,
            face_origin: FaceSketchOrigin::SupportOrigin,
        })
    }

    /// Start a sketch with an explicit coordinate-zero policy for planar
    /// faces. Origin datum planes ignore `face_origin`.
    pub fn begin_sketch_with_options(
        &mut self,
        request: BeginSketchRequest,
    ) -> Result<SketchDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::SketchAlreadyActive);
        }
        let requested_name = request.name.as_deref().map(str::trim);
        if let Some(name) = requested_name {
            if name.is_empty()
                || name.chars().any(char::is_control)
                || self.finished.iter().any(|s| s.session.name() == name)
            {
                return Err(SessionError::Solid(
                    "Sketch name must be non-empty, printable, and unique".into(),
                ));
            }
        }
        let plane = request.plane;
        let basis = match plane {
            PlaneRef::OriginPlane { .. } => plane
                .origin_basis()
                .map_err(|_| SessionError::UnsupportedPlane)?,
            PlaneRef::PlanarFace { face_id } => {
                let mut basis = self.solids.face_basis(face_id).ok_or_else(|| {
                    SessionError::BrokenReference(format!(
                        "face {} no longer exists or is not planar",
                        face_id.0
                    ))
                })?;
                basis.origin = match request.face_origin {
                    FaceSketchOrigin::SupportOrigin => basis.origin,
                    FaceSketchOrigin::FaceCenter => {
                        self.solids.face_center(face_id).unwrap_or(basis.origin)
                    }
                    FaceSketchOrigin::GlobalOriginProjection => {
                        let normal_offset = basis.origin[0] * basis.normal[0]
                            + basis.origin[1] * basis.normal[1]
                            + basis.origin[2] * basis.normal[2];
                        [
                            basis.normal[0] * normal_offset,
                            basis.normal[1] * normal_offset,
                            basis.normal[2] * normal_offset,
                        ]
                    }
                };
                basis
            }
            PlaneRef::DatumPlane { datum_id } => self
                .resolve_datum_basis(datum_id, &self.active_feature_ids())
                .ok_or_else(|| {
                    SessionError::BrokenReference(format!(
                        "construction plane {} is missing or rolled back",
                        datum_id.0
                    ))
                })?,
        };
        self.sketch_count += 1;
        let name = if let Some(name) = requested_name {
            name.to_owned()
        } else {
            while self
                .finished
                .iter()
                .any(|s| s.session.name() == format!("Sketch{}", self.sketch_count))
            {
                self.sketch_count += 1;
            }
            format!("Sketch{}", self.sketch_count)
        };
        self.document.add_browser_child(
            BrowserNodeKind::SketchesFolder,
            BrowserNodeKind::Sketch,
            &name,
        );
        let mut session = SketchSession::new(name, plane, basis, self.grid_snap);
        self.install_support_references(&mut session, plane, basis);

        session.set_grid_snap(self.grid_snap);
        session.set_grid_step(self.grid_step)?;
        let dto = session.dto();
        let feature_id = self.document.alloc_feature_id();
        self.document
            .features_mut()
            .insert_at_rollback(Feature::new(
                feature_id,
                dto.name.clone(),
                FeatureKind::Sketch,
            ));
        self.active_feature_id = Some(feature_id);
        self.active = Some(session);
        self.active_named_view = None;
        Ok(dto)
    }

    /// Finish the active sketch. The sketch stays in the browser tree with
    /// its full session (entities, constraints, dimensions, undo stack) so
    /// it can render in 3D and be re-entered via `edit_sketch` (M1d).
    pub fn end_sketch(&mut self) -> Result<EndSketchResult, SessionError> {
        let mut session = self.active.take().ok_or(SessionError::NoActiveSketch)?;
        session.set_edit_placement(None);
        session.refresh_profile_identities();
        let feature_id = self.active_feature_id.take().ok_or_else(|| {
            SessionError::Solid("active sketch has no history feature".to_string())
        })?;
        self.finished.push(FinishedSketch {
            session,
            feature_id,
        });

        let feature_order = self
            .document
            .features()
            .features
            .iter()
            .enumerate()
            .map(|(index, feature)| (feature.id, index))
            .collect::<HashMap<_, _>>();
        self.finished.sort_by_key(|finished| {
            feature_order
                .get(&finished.feature_id)
                .copied()
                .unwrap_or(usize::MAX)
        });
        Ok(EndSketchResult {
            document: self.document_dto(),
        })
    }

    /// DTOs of every finished sketch (M1d): rendered muted in 3D on their
    /// planes and listed for re-edit.
    pub fn finished_sketches(&self) -> Vec<SketchDto> {
        self.finished.iter().map(|f| f.session.dto()).collect()
    }

    /// Re-enter a finished sketch for editing (M1d): moves the session back
    /// to active, preserving entities, constraints, dimensions, and undo.
    pub fn edit_sketch(&mut self, name: &str) -> Result<SketchDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::SketchAlreadyActive);
        }
        let index = self
            .finished
            .iter()
            .position(|f| f.session.name() == name)
            .ok_or_else(|| SessionError::SketchNotFound(name.to_string()))?;
        let mut f = self.finished.remove(index);
        f.session.set_grid_snap(self.grid_snap);
        f.session.set_grid_step(self.grid_step)?;
        let plane = f.session.plane();
        let basis = f.session.basis();
        if self.scene_matches_history_stage(f.feature_id) {
            self.install_support_references(&mut f.session, plane, basis);
        }
        let dto = f.session.dto();
        self.active_feature_id = Some(f.feature_id);
        self.active = Some(f.session);
        self.active_named_view = None;
        Ok(dto)
    }

    pub fn active_snapshot(&self) -> Option<SketchDto> {
        self.active.as_ref().map(SketchSession::dto)
    }

    pub fn has_active_sketch(&self) -> bool {
        self.active.is_some()
    }

    pub fn profile_catalog(&self) -> Vec<ProfileCatalogItemDto> {
        self.profile_catalog_at(self.document.features().rollback_index)
    }

    /// Build a profile catalog for an explicit history stage. Replays choose
    /// their target stage before committing it to `rollback_index`, so they
    /// cannot rely on the currently displayed stage here.
    fn profile_catalog_at(&self, rollback_index: usize) -> Vec<ProfileCatalogItemDto> {
        let active = self.active_feature_ids_at(rollback_index);
        self.finished
            .iter()
            .filter(|finished| active.contains(&finished.feature_id))
            .map(|finished| finished.session.profile_catalog(finished.feature_id))
            .collect()
    }

    pub fn solid_scene(&self) -> SolidSceneDto {
        self.solids.scene().clone()
    }

    /// Borrow tessellation and topology for synchronous, read-only consumers.
    /// Hosts must keep their engine guard alive while using this reference.
    pub fn solid_scene_ref(&self) -> &SolidSceneDto {
        self.solids.scene()
    }

    /// Share immutable evaluated geometry with an in-process renderer or picker.
    pub fn solid_scene_snapshot(&self) -> std::sync::Arc<SolidSceneDto> {
        self.solids.scene_snapshot()
    }

    pub fn body_appearances(&self) -> Vec<BodyAppearance> {
        self.body_appearances.clone()
    }

    pub fn drawing_document(&self) -> DrawingDocumentDto {
        self.drawings.clone()
    }

    /// Borrow authored sheets for synchronous inspection under the host guard.
    pub fn drawing_document_ref(&self) -> &DrawingDocumentDto {
        &self.drawings
    }

    pub fn assembly_document(&self) -> AssemblyDocumentDto {
        self.assembly.clone()
    }

    /// Borrow assembly intent while a synchronous caller holds the host guard.
    pub fn assembly_document_ref(&self) -> &AssemblyDocumentDto {
        &self.assembly
    }

    pub fn set_assembly_document(
        &mut self,
        mut document: AssemblyDocumentDto,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        document.validate().map_err(SessionError::Solid)?;
        document.component_structure.next_occurrence_id =
            document.component_structure.next_occurrence_id.max(
                print_intent::print_intent_occurrence_floor(&self.print_intent),
            );
        self.assembly = document;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn assembly_solution(&self) -> AssemblySolutionDto {
        if let Some(solution) = self.assembly_solution_cache.borrow().as_ref() {
            return solution.clone();
        }
        let solution = self.assembly.solve(self.solids.scene());
        *self.assembly_solution_cache.borrow_mut() = Some(solution.clone());
        solution
    }

    fn invalidate_assembly_solution(&mut self) {
        self.clear_assembly_solution();
        for sheet in &mut self.drawings.sheets {
            if sheet.release.status == crate::DrawingReleaseStatus::Released
                && sheet
                    .views
                    .iter()
                    .any(|view| view.scope == crate::DrawingViewScope::Assembly)
            {
                sheet.release.status = crate::DrawingReleaseStatus::Draft;
            }
        }
    }

    fn clear_assembly_solution(&mut self) {
        *self.assembly_solution_cache.get_mut() = None;
        self.active_named_view = None;
    }

    pub fn create_component(
        &mut self,
        request: CreateComponentRequestDto,
    ) -> Result<ComponentDefinitionDto, SessionError> {
        self.ensure_no_component_edit()?;
        let component = self
            .assembly
            .create_component(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(component)
    }

    pub fn update_component(
        &mut self,
        request: UpdateComponentRequestDto,
    ) -> Result<ComponentDefinitionDto, SessionError> {
        self.ensure_no_component_edit()?;
        let component = self
            .assembly
            .update_component(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(component)
    }

    pub fn create_occurrence(
        &mut self,
        request: CreateOccurrenceRequestDto,
    ) -> Result<ComponentOccurrenceDto, SessionError> {
        self.ensure_no_component_edit()?;
        let occurrence = self
            .assembly
            .create_occurrence(request)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(occurrence)
    }

    pub fn update_occurrence(
        &mut self,
        request: UpdateOccurrenceRequestDto,
    ) -> Result<ComponentOccurrenceDto, SessionError> {
        self.ensure_no_component_edit()?;
        let occurrence = self
            .assembly
            .update_occurrence(request)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(occurrence)
    }

    pub fn duplicate_occurrence(
        &mut self,
        request: DuplicateOccurrenceRequestDto,
    ) -> Result<ComponentOccurrenceDto, SessionError> {
        self.ensure_no_component_edit()?;
        let occurrence = self
            .assembly
            .duplicate_occurrence_subtree(request)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(occurrence)
    }

    pub fn set_occurrence_grounded(
        &mut self,
        request: SetOccurrenceGroundedRequestDto,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .set_occurrence_grounded(request)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn set_occurrence_pose(
        &mut self,
        request: SetOccurrencePoseRequestDto,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .set_occurrence_pose(request)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn preview_joint(
        &self,
        request: CreateJointRequestDto,
    ) -> Result<AssemblySolutionDto, SessionError> {
        let mut preview = self.assembly.clone();
        preview
            .create(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        Ok(preview.solve(self.solids.scene()))
    }

    pub fn create_joint(
        &mut self,
        request: CreateJointRequestDto,
    ) -> Result<JointDefinitionDto, SessionError> {
        self.ensure_no_component_edit()?;
        let joint = self
            .assembly
            .create(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(joint)
    }

    pub fn delete_joint(&mut self, id: JointId) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly.delete(id).map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn update_joint(
        &mut self,
        request: UpdateJointRequestDto,
    ) -> Result<JointDefinitionDto, SessionError> {
        self.ensure_no_component_edit()?;
        let joint = self
            .assembly
            .update(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(joint)
    }

    pub fn preview_joint_update(
        &self,
        request: UpdateJointRequestDto,
    ) -> Result<AssemblySolutionDto, SessionError> {
        let mut preview = self.assembly.clone();
        preview
            .update(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        Ok(preview.solve(self.solids.scene()))
    }

    pub fn set_joint_enabled(
        &mut self,
        request: SetJointEnabledRequestDto,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .set_joint_enabled(request.joint_id, request.enabled)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn set_joint_motion(
        &mut self,
        request: SetJointMotionRequestDto,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .drive_joint_motion(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    /// Solve a candidate joint position without mutating the saved assembly.
    /// Viewport animation and direct manipulation must cross this boundary so
    /// a preview can never dirty the project or silently become a design pose.
    pub fn preview_joint_motion(
        &self,
        request: SetJointMotionRequestDto,
    ) -> Result<AssemblySolutionDto, SessionError> {
        let mut preview = self.assembly.clone();
        preview
            .drive_joint_motion(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        Ok(preview.solve(self.solids.scene()))
    }

    pub fn set_joint_coordinates(
        &mut self,
        request: SetJointCoordinatesRequestDto,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .drive_joint_coordinates(request.motion, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    /// Solve a complete (possibly multi-axis) joint coordinate state without
    /// mutating the saved assembly document.
    pub fn preview_joint_coordinates(
        &self,
        request: SetJointCoordinatesRequestDto,
    ) -> Result<AssemblySolutionDto, SessionError> {
        let mut preview = self.assembly.clone();
        preview
            .drive_joint_coordinates(request.motion, self.solids.scene())
            .map_err(SessionError::Solid)?;
        Ok(preview.solve(self.solids.scene()))
    }

    pub fn create_gear_relation(
        &mut self,
        request: CreateGearRelationRequestDto,
    ) -> Result<GearRelationDto, SessionError> {
        self.ensure_no_component_edit()?;
        let relation = self
            .assembly
            .create_gear_relation(request, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(relation)
    }

    pub fn update_gear_relation(
        &mut self,
        relation: GearRelationDto,
    ) -> Result<GearRelationDto, SessionError> {
        self.ensure_no_component_edit()?;
        let relation = self
            .assembly
            .update_gear_relation(relation, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(relation)
    }

    pub fn delete_gear_relation(&mut self, id: u64) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .delete_gear_relation(id)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn preview_mechanism_drag(
        &self,
        request: MechanismDragRequestDto,
    ) -> Result<MechanismPreviewDto, SessionError> {
        self.assembly
            .preview_mechanism_drag(request, self.solids.scene())
            .map_err(SessionError::Solid)
    }

    pub fn apply_joint_motions(
        &mut self,
        request: ApplyJointMotionsRequestDto,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .apply_joint_motions(&request.motions)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn create_assembly_position(
        &mut self,
        request: CreateAssemblyPositionRequestDto,
    ) -> Result<AssemblyPositionDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .create_position(request)
            .map_err(SessionError::Solid)
    }

    pub fn update_assembly_position(
        &mut self,
        position: AssemblyPositionDto,
    ) -> Result<AssemblyPositionDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .update_position(position)
            .map_err(SessionError::Solid)
    }

    pub fn delete_assembly_position(
        &mut self,
        id: AssemblyPositionId,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .delete_position(id)
            .map_err(SessionError::Solid)?;
        Ok(self.assembly.clone())
    }

    pub fn apply_assembly_position(
        &mut self,
        id: AssemblyPositionId,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .apply_position(id)
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn create_motion_study(
        &mut self,
        request: CreateMotionStudyRequestDto,
    ) -> Result<MotionStudyDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .create_motion_study(request)
            .map_err(SessionError::Solid)
    }

    pub fn update_motion_study(
        &mut self,
        study: MotionStudyDto,
    ) -> Result<MotionStudyDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .update_motion_study(study)
            .map_err(SessionError::Solid)
    }

    pub fn delete_motion_study(
        &mut self,
        id: MotionStudyId,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .delete_motion_study(id)
            .map_err(SessionError::Solid)?;
        Ok(self.assembly.clone())
    }

    pub fn sample_motion_study(
        &self,
        request: SampleMotionStudyRequestDto,
    ) -> Result<MotionStudySampleDto, SessionError> {
        self.assembly
            .sample_motion_study(request, self.solids.scene())
            .map_err(SessionError::Solid)
    }

    pub fn export_motion_path_csv(
        &self,
        request: MotionPathRequestDto,
    ) -> Result<String, SessionError> {
        self.assembly
            .export_motion_path_csv(request, self.solids.scene())
            .map_err(SessionError::Solid)
    }

    pub fn create_contact_set(
        &mut self,
        request: CreateContactSetRequestDto,
    ) -> Result<ContactSetDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .create_contact_set(request)
            .map_err(SessionError::Solid)
    }

    pub fn update_contact_set(
        &mut self,
        contact: ContactSetDto,
    ) -> Result<ContactSetDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .update_contact_set(contact)
            .map_err(SessionError::Solid)
    }

    pub fn delete_contact_set(
        &mut self,
        id: ContactSetId,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .delete_contact_set(id)
            .map_err(SessionError::Solid)?;
        Ok(self.assembly.clone())
    }

    pub fn approximate_interference_check(
        &self,
        request: InterferenceCheckRequestDto,
    ) -> Result<InterferenceReportDto, SessionError> {
        approximate_interference_report(
            self.solids.scene(),
            &self.assembly_solution().instance_body_poses,
            &request,
        )
        .map_err(SessionError::Solid)
    }

    pub fn approximate_motion_study_evaluation(
        &self,
        request: EvaluateMotionStudyRequestDto,
    ) -> Result<MotionStudyEvaluationDto, SessionError> {
        let mut sample = self.sample_motion_study(SampleMotionStudyRequestDto {
            study_id: request.study_id,
            time_seconds: request.time_seconds,
        })?;
        let mut stopped_by_contact = None;
        let mut stop_time_seconds = None;
        if request.enforce_contacts {
            let start = self.sample_motion_study(SampleMotionStudyRequestDto {
                study_id: request.study_id,
                time_seconds: request.previous_time_seconds.unwrap_or(0.0),
            })?;
            for contact in self
                .assembly
                .contact_sets
                .iter()
                .filter(|contact| contact.enabled && contact.stop_motion)
            {
                let violation = |candidate: &MotionStudySampleDto| -> Result<f64, SessionError> {
                    let a = candidate
                        .solution
                        .instance_body_poses
                        .iter()
                        .find(|pose| {
                            pose.occurrence_id == contact.occurrence_a
                                && pose.body_id == contact.body_a
                        })
                        .ok_or_else(|| {
                            SessionError::Solid(format!(
                                "contact '{}' first body is missing",
                                contact.name
                            ))
                        })?;
                    let b = candidate
                        .solution
                        .instance_body_poses
                        .iter()
                        .find(|pose| {
                            pose.occurrence_id == contact.occurrence_b
                                && pose.body_id == contact.body_b
                        })
                        .ok_or_else(|| {
                            SessionError::Solid(format!(
                                "contact '{}' second body is missing",
                                contact.name
                            ))
                        })?;
                    let pair =
                        approximate_pair_result(self.solids.scene(), a, b, contact.clearance_mm)
                            .map_err(SessionError::Solid)?;
                    Ok(contact_violation_score(&pair, contact.clearance_mm))
                };
                let start_violation = violation(&start)?;
                let end_violation = violation(&sample)?;
                if start_violation > 1.0e-7 && end_violation >= start_violation {
                    sample = start;
                    stopped_by_contact = Some(contact.id);
                    stop_time_seconds = Some(sample.time_seconds);
                    break;
                }
                if start_violation <= 1.0e-7 {
                    const PROBE_STEPS: usize = 8;
                    let end = sample.clone();
                    let mut safe_time = start.time_seconds;
                    let mut crossing = None;
                    for step in 1..=PROBE_STEPS {
                        let fraction = step as f64 / PROBE_STEPS as f64;
                        let time =
                            start.time_seconds + (end.time_seconds - start.time_seconds) * fraction;
                        let candidate = if step == PROBE_STEPS {
                            end.clone()
                        } else {
                            self.sample_motion_study(SampleMotionStudyRequestDto {
                                study_id: request.study_id,
                                time_seconds: time,
                            })?
                        };
                        let candidate_violation = if step == PROBE_STEPS {
                            end_violation
                        } else {
                            violation(&candidate)?
                        };
                        if candidate_violation <= 1.0e-7 {
                            safe_time = candidate.time_seconds;
                            continue;
                        }
                        let mut safe = safe_time;
                        let mut blocked = candidate.time_seconds;
                        for _ in 0..16 {
                            let middle = (safe + blocked) * 0.5;
                            let middle_sample =
                                self.sample_motion_study(SampleMotionStudyRequestDto {
                                    study_id: request.study_id,
                                    time_seconds: middle,
                                })?;
                            if violation(&middle_sample)? > 1.0e-7 {
                                blocked = middle;
                            } else {
                                safe = middle;
                            }
                        }
                        crossing = Some(blocked);
                        break;
                    }
                    if let Some(blocked) = crossing {
                        sample = self.sample_motion_study(SampleMotionStudyRequestDto {
                            study_id: request.study_id,
                            time_seconds: blocked,
                        })?;
                        stopped_by_contact = Some(contact.id);
                        stop_time_seconds = Some(blocked);
                        break;
                    }
                }
            }
        }
        let mut contact_pairs = Vec::new();
        for contact in self
            .assembly
            .contact_sets
            .iter()
            .filter(|contact| contact.enabled)
        {
            let a = sample
                .solution
                .instance_body_poses
                .iter()
                .find(|pose| {
                    pose.occurrence_id == contact.occurrence_a && pose.body_id == contact.body_a
                })
                .ok_or_else(|| {
                    SessionError::Solid(format!("contact '{}' first body is missing", contact.name))
                })?;
            let b = sample
                .solution
                .instance_body_poses
                .iter()
                .find(|pose| {
                    pose.occurrence_id == contact.occurrence_b && pose.body_id == contact.body_b
                })
                .ok_or_else(|| {
                    SessionError::Solid(format!(
                        "contact '{}' second body is missing",
                        contact.name
                    ))
                })?;
            contact_pairs.push(
                approximate_pair_result(self.solids.scene(), a, b, contact.clearance_mm)
                    .map_err(SessionError::Solid)?,
            );
        }
        let contacts = InterferenceReportDto {
            exact: false,
            pairs: contact_pairs,
        };
        Ok(MotionStudyEvaluationDto {
            sample,
            contacts,
            stopped_by_contact,
            stop_time_seconds,
        })
    }

    pub fn approximate_swept_collision_check(
        &self,
        request: SweptCollisionRequestDto,
    ) -> Result<SweptCollisionReportDto, SessionError> {
        if !request.sample_rate_hz.is_finite() || !(1.0..=240.0).contains(&request.sample_rate_hz) {
            return Err(SessionError::Solid(
                "swept collision sample rate must be between 1 and 240 Hz".to_string(),
            ));
        }
        let study = self
            .assembly
            .motion_studies
            .iter()
            .find(|study| study.id == request.study_id)
            .ok_or_else(|| {
                SessionError::Solid(format!(
                    "motion study {} does not exist",
                    request.study_id.0
                ))
            })?;
        let count = (study.duration_seconds * request.sample_rate_hz).ceil() as u32 + 1;
        let mut events = HashMap::<(u64, u64, u64, u64), SweptCollisionEventDto>::new();
        for index in 0..count {
            let time = (index as f64 / request.sample_rate_hz).min(study.duration_seconds);
            let sample = self.sample_motion_study(SampleMotionStudyRequestDto {
                study_id: request.study_id,
                time_seconds: time,
            })?;
            let report = approximate_interference_report(
                self.solids.scene(),
                &sample.solution.instance_body_poses,
                &InterferenceCheckRequestDto {
                    occurrence_ids: Vec::new(),
                    clearance_threshold_mm: request.clearance_threshold_mm,
                },
            )
            .map_err(SessionError::Solid)?;
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
                return Ok(SweptCollisionReportDto {
                    exact: false,
                    sample_count: index + 1,
                    events: result,
                });
            }
        }
        let mut result = events.into_values().collect::<Vec<_>>();
        result.sort_by(|a, b| a.first_time_seconds.total_cmp(&b.first_time_seconds));
        Ok(SweptCollisionReportDto {
            exact: false,
            sample_count: count,
            events: result,
        })
    }

    pub fn set_grounded_body(
        &mut self,
        body_id: Option<limo_cad_core::BodyId>,
    ) -> Result<AssemblyDocumentDto, SessionError> {
        self.ensure_no_component_edit()?;
        self.assembly
            .set_grounded_body(body_id, self.solids.scene())
            .map_err(SessionError::Solid)?;
        self.invalidate_assembly_solution();
        Ok(self.assembly.clone())
    }

    pub fn project_visibility(&self) -> ProjectVisibilityDto {
        self.scrubbed_project_visibility()
    }

    pub fn set_project_visibility(
        &mut self,
        visibility: ProjectVisibilityDto,
    ) -> Result<ProjectVisibilityDto, SessionError> {
        self.project_visibility = visibility;
        self.scrub_project_visibility();
        Ok(self.project_visibility.clone())
    }

    pub fn set_construction_visibility(
        &mut self,
        request: crate::ConstructionVisibilityRequest,
    ) -> Result<ProjectVisibilityDto, SessionError> {
        let retained_sketches = self
            .finished
            .iter()
            .map(|sketch| sketch.session.name().to_string())
            .collect::<BTreeSet<_>>();
        let retained_datums = self
            .datum_planes
            .iter()
            .map(|plane| plane.datum_id.0)
            .collect::<BTreeSet<_>>();
        let all = request.sketch_names.is_none() && request.datum_plane_ids.is_none();
        let sketches = if all {
            retained_sketches.clone()
        } else {
            request
                .sketch_names
                .unwrap_or_default()
                .into_iter()
                .collect()
        };
        let datums = if all {
            retained_datums.clone()
        } else {
            request
                .datum_plane_ids
                .unwrap_or_default()
                .into_iter()
                .collect()
        };

        if let Some(name) = sketches.difference(&retained_sketches).next() {
            return Err(SessionError::Solid(format!(
                "Retained sketch '{name}' was not found"
            )));
        }
        if let Some(id) = datums.difference(&retained_datums).next() {
            return Err(SessionError::Solid(format!(
                "Datum plane {id} was not found"
            )));
        }
        let mut visibility = self.project_visibility();
        let mut hidden_sketches = visibility
            .hidden_sketch_names
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut hidden_datums = visibility
            .hidden_datum_plane_ids
            .into_iter()
            .collect::<BTreeSet<_>>();
        if request.visible {
            hidden_sketches.retain(|name| !sketches.contains(name));
            hidden_datums.retain(|id| !datums.contains(id));
        } else {
            hidden_sketches.extend(sketches);
            hidden_datums.extend(datums);
        }
        visibility.hidden_sketch_names = hidden_sketches.into_iter().collect();
        visibility.hidden_datum_plane_ids = hidden_datums.into_iter().collect();
        self.set_project_visibility(visibility)
    }

    pub fn named_views(&self) -> NamedViewsDto {
        NamedViewsDto {
            views: self.scrubbed_named_views(),
            active: self.active_named_view.clone(),
        }
    }

    /// Return to assembled display without editing saved views or visibility.
    pub fn clear_named_view(&mut self) -> NamedViewsDto {
        self.active_named_view = None;
        self.named_views()
    }

    /// Create or replace one view atomically, preserving other configurations.
    pub fn upsert_named_view(
        &mut self,
        view: NamedViewConfigurationDto,
    ) -> Result<NamedViewsDto, SessionError> {
        let mut views = self.scrubbed_named_views();
        if let Some(existing) = views.iter_mut().find(|existing| existing.name == view.name) {
            *existing = view;
        } else {
            views.push(view);
        }
        self.set_named_views(views)
    }

    pub fn rename_named_view(
        &mut self,
        name: String,
        new_name: String,
    ) -> Result<NamedViewsDto, SessionError> {
        let mut views = self.scrubbed_named_views();
        let view = views
            .iter_mut()
            .find(|view| view.name == name)
            .ok_or_else(|| SessionError::Solid(format!("Named view '{name}' was not found")))?;
        view.name = new_name;
        self.set_named_views(views)
    }

    pub fn delete_named_view(&mut self, name: String) -> Result<NamedViewsDto, SessionError> {
        let mut views = self.scrubbed_named_views();
        let index = views
            .iter()
            .position(|view| view.name == name)
            .ok_or_else(|| SessionError::Solid(format!("Named view '{name}' was not found")))?;
        views.remove(index);
        self.set_named_views(views)
    }

    /// Replace the saved review views. Unknown bodies reject the whole list.
    /// Solid definitions are not modified.
    pub fn set_named_views(
        &mut self,
        mut views: Vec<NamedViewConfigurationDto>,
    ) -> Result<NamedViewsDto, SessionError> {
        if self.pending_project.is_some() {
            return Err(SessionError::Solid(
                "Named layouts cannot change during project replacement".into(),
            ));
        }
        self.solids
            .ensure_metadata_editable()
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        for view in &mut views {
            view.visible_body_ids.sort_unstable();
            view.visible_body_ids.dedup();
            view.part_offsets.sort_by_key(|offset| offset.body_id);
            view.occurrence_offsets
                .sort_by_key(|offset| offset.occurrence_id.0);
            for offset in &view.occurrence_offsets {
                if !self
                    .assembly
                    .component_structure
                    .occurrences
                    .iter()
                    .any(|o| o.id == offset.occurrence_id)
                {
                    return Err(SessionError::Solid(format!(
                        "Named view '{}' references unknown occurrence {}",
                        view.name, offset.occurrence_id.0
                    )));
                }
            }
        }
        crate::dto::validate_named_views(&views).map_err(SessionError::Solid)?;
        let retained = self.retained_presentation_body_ids();
        for view in &views {
            for id in view
                .visible_body_ids
                .iter()
                .copied()
                .chain(view.part_offsets.iter().map(|offset| offset.body_id))
            {
                if !retained.contains(&limo_cad_core::BodyId(id)) {
                    return Err(SessionError::Solid(format!("Body {id} was not found")));
                }
            }
        }
        for view in &mut views {
            if let Some(existing) = self
                .named_views
                .iter()
                .find(|existing| existing.name == view.name)
            {
                if view.id.is_some() && view.id != existing.id {
                    return Err(SessionError::Solid(
                        "A saved layout identity cannot be replaced".into(),
                    ));
                }
                view.id = existing.id.clone();
            } else if view.id.as_ref().is_some_and(|id| {
                !self
                    .named_views
                    .iter()
                    .any(|existing| existing.id.as_ref() == Some(id))
            }) {
                return Err(SessionError::Solid(
                    "New layouts cannot adopt an unknown saved identity".into(),
                ));
            }
            if view.id.is_none() {
                view.id = Some(uuid::Uuid::new_v4().to_string());
            }
        }
        crate::dto::validate_named_views(&views).map_err(SessionError::Solid)?;
        self.named_views = views;
        self.active_named_view = None;
        self.sync_named_view_browser();
        Ok(self.named_views())
    }

    /// Apply a saved view's body visibility. The camera and part offsets are
    /// returned for the viewport; neither is written into solid geometry.
    pub fn recall_named_view(&mut self, name: String) -> Result<RecallNamedViewDto, SessionError> {
        self.ensure_no_active_sketch("recalling a named view")?;
        let name = name.trim();
        if name.is_empty() {
            return Err(SessionError::Solid(
                "named view name cannot be empty".to_string(),
            ));
        }
        let view = self
            .scrubbed_named_views()
            .into_iter()
            .find(|view| view.name == name)
            .ok_or_else(|| SessionError::Solid(format!("Named view '{name}' was not found")))?;
        let solution = self.resolve_named_view(&view)?;
        let retained = self.retained_presentation_body_ids();
        let visible = view
            .visible_body_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut visibility = self.project_visibility();
        visibility.hidden_body_ids = retained
            .iter()
            .map(|id| id.0)
            .filter(|id| !visible.contains(id))
            .collect();
        let visibility = self.set_project_visibility(visibility)?;
        self.active_named_view = Some(view.name.clone());
        Ok(RecallNamedViewDto {
            view,
            visibility,
            solution,
        })
    }

    /// One read-only layout resolver for display, STL and 3MF. Camera has no
    /// influence on geometry; named-view offsets never modify the assembly.
    pub fn named_view_solution(
        &self,
        name: Option<&str>,
    ) -> Result<AssemblySolutionDto, SessionError> {
        if let Some(name) = name {
            let view = self
                .named_views()
                .views
                .into_iter()
                .find(|v| v.name == name)
                .ok_or_else(|| SessionError::Solid(format!("Named view '{name}' was not found")))?;
            return self.resolve_named_view(&view);
        }
        let mut solution = self.assembly_solution();
        let hidden: BTreeSet<_> = self
            .project_visibility()
            .hidden_body_ids
            .into_iter()
            .collect();
        for pose in &mut solution.instance_body_poses {
            pose.visible &= !hidden.contains(&pose.body_id.0);
        }
        Ok(solution)
    }

    /// Resolve the recalled display with the live Browser eye-toggle choices.
    /// Saved visibility is a recall snapshot; toggling an eye afterwards does
    /// not edit that snapshot or discard its occurrence placement.
    pub fn presentation_solution(&self) -> Result<AssemblySolutionDto, SessionError> {
        let Some(name) = self.active_named_view.as_deref() else {
            return self.named_view_solution(None);
        };
        let mut view = self
            .scrubbed_named_views()
            .into_iter()
            .find(|view| view.name == name)
            .ok_or_else(|| SessionError::Solid(format!("Named view '{name}' was not found")))?;
        let hidden: BTreeSet<_> = self
            .project_visibility()
            .hidden_body_ids
            .into_iter()
            .collect();
        view.visible_body_ids = self
            .retained_presentation_body_ids()
            .into_iter()
            .map(|id| id.0)
            .filter(|id| !hidden.contains(id))
            .collect();
        self.resolve_named_view(&view)
    }

    /// Export selection: absent uses the current display; an empty name uses
    /// assembled placement and live visibility; a name uses its saved snapshot.
    pub fn export_view_solution(
        &self,
        name: Option<&str>,
    ) -> Result<AssemblySolutionDto, SessionError> {
        match name {
            None => self.presentation_solution(),
            Some("") => self.named_view_solution(None),
            Some(name) => self.named_view_solution(Some(name)),
        }
    }

    /// The envelope follows the selected display or saved layout unless the
    /// caller explicitly overrides it. An assembled export uses the default bed.
    pub fn export_print_bed(
        &self,
        name: Option<&str>,
    ) -> Result<limo_cad_core::PrintBedDto, SessionError> {
        let name = name
            .or(self.active_named_view.as_deref())
            .filter(|name| !name.is_empty());
        match name {
            None => Ok(Default::default()),
            Some(name) => self
                .scrubbed_named_views()
                .into_iter()
                .find(|view| view.name == name)
                .map(|view| view.print_bed)
                .ok_or_else(|| SessionError::Solid(format!("Named view '{name}' was not found"))),
        }
    }

    pub fn resolve_named_view(
        &self,
        view: &NamedViewConfigurationDto,
    ) -> Result<AssemblySolutionDto, SessionError> {
        crate::dto::validate_named_views(std::slice::from_ref(view))
            .map_err(SessionError::Solid)?;
        let mut solution = limo_cad_assembly::resolve_view_layout(
            &self.assembly.component_structure,
            &self.assembly_solution(),
            &view.occurrence_offsets,
        )
        .map_err(SessionError::Solid)?;
        let visible: BTreeSet<_> = view.visible_body_ids.iter().copied().collect();
        for pose in &mut solution.instance_body_poses {
            pose.visible &= visible.contains(&pose.body_id.0);
            if let Some(offset) = view
                .part_offsets
                .iter()
                .find(|o| o.body_id == pose.body_id.0)
            {
                for axis in 0..3 {
                    pose.translation[axis] += offset.translation[axis];
                }
            }
        }
        for pose in &mut solution.body_poses {
            if let Some(instance) = solution
                .instance_body_poses
                .iter()
                .find(|p| p.body_id == pose.body_id)
            {
                pose.translation = instance.translation;
                pose.rotation = instance.rotation;
            }
        }
        Ok(solution)
    }

    pub fn set_drawing_document(
        &mut self,
        drawing: DrawingDocumentDto,
    ) -> Result<DrawingDocumentDto, SessionError> {
        drawing.validate().map_err(SessionError::Solid)?;
        self.drawings = crate::drawing_topology::capture_drawing_topology(
            drawing,
            self.solid_scene_ref(),
            Some(&self.drawings),
        )
        .map_err(SessionError::Solid)?;
        Ok(self.drawings.clone())
    }

    pub fn geometry_edge_chain(
        &self,
        request: crate::EdgeChainRequest,
    ) -> Result<limo_cad_core::edge_chain::Chain, SessionError> {
        let sketches = if request.source == crate::ChainSource::Sketch {
            self.finished_sketches()
        } else {
            Vec::new()
        };
        crate::edge_selection::resolve(self.solids.scene(), &sketches, &request)
            .map_err(SessionError::Solid)
    }

    pub fn cam_chamfer_geometry(
        &self,
        request: crate::CamChamferGeometryRequest,
    ) -> Result<crate::CamChamferGeometry, SessionError> {
        let setup = self
            .cam
            .setups
            .iter()
            .find(|s| s.id == request.setup_id)
            .ok_or_else(|| SessionError::Solid("The CAM setup no longer exists.".into()))?;
        crate::cam_chamfer::resolve(self.solids.scene(), setup, &request.chain_ref)
            .map_err(SessionError::Solid)
    }

    pub fn cam_document(&self) -> CamDocumentDto {
        self.cam.clone()
    }

    pub fn set_cam_document(
        &mut self,
        mut cam: CamDocumentDto,
    ) -> Result<CamDocumentDto, SessionError> {
        cam.migrate_legacy();
        cam.validate_for_editing().map_err(SessionError::Solid)?;

        cam.refresh_load_warnings();
        self.upgrade_verified_legacy_cam_generations(&mut cam);
        self.cam = cam;
        Ok(self.cam.clone())
    }

    /// Translate old full-prefix stamps against the PRE-EDIT document only.
    /// This avoids forcing a one-time regeneration for an unchanged reviewed
    /// job, without blessing a stale stamp or the reordered/edited inputs.
    fn upgrade_verified_legacy_cam_generations(&self, next: &mut CamDocumentDto) {
        if !next
            .toolpath_generations
            .iter()
            .any(|s| s.order_dependencies.is_none())
        {
            return;
        }
        for setup in &self.cam.setups {
            let Ok(dependencies) = self.cam_setup_dependency_fingerprints(setup) else {
                continue;
            };
            for operation in setup.operations.iter().filter(|o| o.enabled()) {
                let Some(saved) =
                    self.cam.toolpath_generations.iter().find(|s| {
                        s.operation_id == operation.id() && s.order_dependencies.is_none()
                    })
                else {
                    continue;
                };
                let Some(incoming) = next.toolpath_generations.iter_mut().find(|s| *s == saved)
                else {
                    continue;
                };
                let Ok(legacy) =
                    self.cam_generation_signature(setup, operation, &dependencies, true)
                else {
                    continue;
                };
                if *saved == legacy {
                    if let Ok(upgraded) =
                        self.current_cam_generation(setup, operation, &dependencies)
                    {
                        *incoming = upgraded;
                    }
                }
            }
        }
    }

    fn cam_setup_dependency_fingerprints(
        &self,
        setup: &CamSetupDto,
    ) -> Result<CamSetupDependencyFingerprints, SessionError> {
        let mut setup_intent = setup.clone();
        setup_intent.operations.clear();

        setup_intent.machine = None;

        let mut body_ids = BTreeSet::new();
        let mut upstream_setups = Vec::new();
        let mut visited = BTreeSet::from([setup.id]);
        let mut cursor = setup;
        loop {
            body_ids.extend(cursor.body_ids.iter().copied());
            if let CamResolvedStockDto::ModelBody { body_id } = &cursor.resolved_stock {
                body_ids.insert(BodyId(*body_id));
            }
            let CamResolvedStockDto::Rest { source_setup_id } = &cursor.resolved_stock else {
                break;
            };
            let source_setup_id = *source_setup_id;
            if !visited.insert(source_setup_id) {
                return Err(SessionError::Solid(
                    "CAM rest-stock dependency contains a cycle".to_string(),
                ));
            }
            let source = self
                .cam
                .setups
                .iter()
                .find(|candidate| candidate.id == source_setup_id)
                .ok_or_else(|| {
                    SessionError::Solid(format!(
                        "CAM rest-stock source setup {source_setup_id} no longer exists"
                    ))
                })?;
            let mut upstream_intent = source.clone();
            upstream_intent.machine = None;
            upstream_setups.push(upstream_intent);
            cursor = source;
        }

        let bodies = self
            .solids
            .scene()
            .bodies
            .iter()
            .filter(|body| body_ids.contains(&body.id))
            .collect::<Vec<_>>();

        let sketches = self
            .finished
            .iter()
            .map(|finished| {
                let mut sketch = finished.session.dto();
                sketch.can_undo = false;
                sketch.can_redo = false;
                sketch
            })
            .collect::<Vec<_>>();
        let upstream_tool_ids = upstream_setups
            .iter()
            .flat_map(|source| source.operations.iter().map(CamOperationDto::tool_id))
            .collect::<BTreeSet<_>>();
        let upstream_tools = self
            .cam
            .tools
            .iter()
            .filter(|tool| upstream_tool_ids.contains(&tool.id))
            .collect::<Vec<_>>();
        let upstream_operation_ids = upstream_setups
            .iter()
            .flat_map(|s| s.operations.iter().map(CamOperationDto::id))
            .collect::<BTreeSet<_>>();
        let upstream_linking = self
            .cam
            .linking
            .iter()
            .filter(|l| upstream_operation_ids.contains(&l.operation_id))
            .collect::<Vec<_>>();

        Ok(CamSetupDependencyFingerprints {
            model: stable_cam_fingerprint(&(
                "cam-model-dependencies",
                CAM_TOOLPATH_PLANNER_REVISION,
                &body_ids,
                bodies,
                sketches,
            ))?,
            setup: stable_cam_fingerprint(&(
                "cam-setup-dependencies",
                CAM_TOOLPATH_PLANNER_REVISION,
                setup_intent,
            ))?,
            upstream: stable_cam_fingerprint(&(
                "cam-upstream-dependencies",
                CAM_TOOLPATH_PLANNER_REVISION,
                upstream_setups,
                upstream_tools,
                upstream_linking,
            ))?,
        })
    }

    fn current_cam_generation(
        &self,
        setup: &CamSetupDto,
        operation: &CamOperationDto,
        dependencies: &CamSetupDependencyFingerprints,
    ) -> Result<CamToolpathGenerationDto, SessionError> {
        self.cam_generation_signature(setup, operation, dependencies, false)
    }

    fn cam_generation_signature(
        &self,
        setup: &CamSetupDto,
        operation: &CamOperationDto,
        dependencies: &CamSetupDependencyFingerprints,
        legacy_prefix: bool,
    ) -> Result<CamToolpathGenerationDto, SessionError> {
        let tool = self
            .cam
            .tools
            .iter()
            .find(|candidate| candidate.id == operation.tool_id())
            .ok_or_else(|| {
                SessionError::Solid(format!(
                    "operation '{}' references a missing tool",
                    operation.name()
                ))
            })?;
        let height_expressions = self
            .cam
            .height_expressions
            .iter()
            .find(|expressions| expressions.operation_id == operation.id());
        let linking = self
            .cam
            .linking
            .iter()
            .find(|item| item.operation_id == operation.id());
        let source_inputs = |o: &CamOperationDto| {
            (
                o.clone(),
                self.cam.tool(o.tool_id()),
                self.cam
                    .height_expressions
                    .iter()
                    .find(|h| h.operation_id == o.id()),
                self.cam.linking.iter().find(|l| l.operation_id == o.id()),
            )
        };
        let operation_fingerprint = if legacy_prefix {
            let prefix = setup
                .operations
                .iter()
                .take_while(|o| o.id() != operation.id())
                .filter(|o| o.enabled())
                .map(source_inputs)
                .collect::<Vec<_>>();
            stable_cam_fingerprint(&(
                "cam-operation-dependencies",
                CAM_TOOLPATH_PLANNER_REVISION,
                operation,
                height_expressions,
                linking,
                prefix,
            ))?
        } else {
            stable_cam_fingerprint(&(
                "cam-operation-intent",
                CAM_TOOLPATH_PLANNER_REVISION,
                operation,
                height_expressions,
                linking,
            ))?
        };
        let order_dependencies = if legacy_prefix {
            None
        } else {
            let rules = limo_cad_cam::cam_operation_dependencies(setup, operation, linking);
            let fingerprint = |kind| {
                let sources = rules
                    .iter()
                    .filter(|d| d.kind == kind)
                    .filter_map(|d| setup.operations.iter().find(|o| o.id() == d.operation_id))
                    .map(source_inputs)
                    .collect::<Vec<_>>();
                stable_cam_fingerprint(&(
                    "cam-order-evidence",
                    limo_cad_cam::CAM_ORDER_DEPENDENCY_RULES_REVISION,
                    sources,
                ))
            };
            Some(limo_cad_cam::CamToolpathOrderDependenciesDto {
                rules_revision: limo_cad_cam::CAM_ORDER_DEPENDENCY_RULES_REVISION,
                stock_height_fingerprint: fingerprint(
                    limo_cad_cam::CamOperationDependencyKind::IncomingStockHeight,
                )?,
                predrill_fingerprint: fingerprint(
                    limo_cad_cam::CamOperationDependencyKind::PredrilledEntry,
                )?,
            })
        };
        Ok(CamToolpathGenerationDto {
            operation_id: operation.id(),
            planner_revision: CAM_TOOLPATH_PLANNER_REVISION,
            model_fingerprint: dependencies.model.clone(),
            setup_fingerprint: dependencies.setup.clone(),
            operation_fingerprint,
            tool_fingerprint: stable_cam_fingerprint(&(
                "cam-tool-dependencies",
                CAM_TOOLPATH_PLANNER_REVISION,
                tool,
            ))?,
            upstream_fingerprint: dependencies.upstream.clone(),
            order_dependencies,
        })
    }

    fn ensure_cam_model_references_exist(&self, setup: &CamSetupDto) -> Result<(), SessionError> {
        let available_body_ids = self
            .solids
            .scene()
            .bodies
            .iter()
            .map(|body| body.id)
            .collect::<BTreeSet<_>>();
        let mut required_body_ids = BTreeSet::new();
        let mut visited = BTreeSet::from([setup.id]);
        let mut cursor = setup;
        loop {
            required_body_ids.extend(cursor.body_ids.iter().copied());
            if let CamResolvedStockDto::ModelBody { body_id } = &cursor.resolved_stock {
                required_body_ids.insert(BodyId(*body_id));
            }
            let CamResolvedStockDto::Rest { source_setup_id } = &cursor.resolved_stock else {
                break;
            };
            let source_setup_id = *source_setup_id;
            if !visited.insert(source_setup_id) {
                return Err(SessionError::Solid(
                    "CAM rest-stock dependency contains a cycle".to_string(),
                ));
            }
            cursor = self
                .cam
                .setups
                .iter()
                .find(|candidate| candidate.id == source_setup_id)
                .ok_or_else(|| {
                    SessionError::Solid(format!(
                        "CAM rest-stock source setup {source_setup_id} no longer exists"
                    ))
                })?;
        }
        let missing = required_body_ids
            .difference(&available_body_ids)
            .map(|id| id.0.to_string())
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        Err(SessionError::Solid(format!(
            "Cannot regenerate setup '{}': referenced CAD bod{} {} no longer exist{}. Repair the setup/model selection first.",
            setup.name,
            if missing.len() == 1 { "y" } else { "ies" },
            missing.join(", "),
            if missing.len() == 1 { "s" } else { "" },
        )))
    }

    pub fn cam_toolpath_statuses(&self) -> Result<Vec<CamToolpathStatusDto>, SessionError> {
        let mut statuses = Vec::new();
        for setup in &self.cam.setups {
            if !setup.operations.iter().any(CamOperationDto::enabled) {
                continue;
            }
            let dependencies = self.cam_setup_dependency_fingerprints(setup)?;
            for operation in setup
                .operations
                .iter()
                .filter(|operation| operation.enabled())
            {
                if let Err(reason) = operation.validate(setup, &self.cam.tools) {
                    statuses.push(CamToolpathStatusDto {
                        setup_id: setup.id,
                        operation_id: operation.id(),
                        state: CamToolpathStateDto::Invalid,
                        reasons: vec![reason],
                    });
                    continue;
                }
                let saved = self
                    .cam
                    .toolpath_generations
                    .iter()
                    .find(|generation| generation.operation_id == operation.id());
                let Some(saved) = saved else {
                    statuses.push(CamToolpathStatusDto {
                        setup_id: setup.id,
                        operation_id: operation.id(),
                        state: CamToolpathStateDto::NeverGenerated,
                        reasons: vec![
                            "Toolpath has not been regenerated and checked for this project version."
                                .to_string(),
                        ],
                    });
                    continue;
                };
                let current = self.cam_generation_signature(
                    setup,
                    operation,
                    &dependencies,
                    saved.order_dependencies.is_none(),
                )?;
                let mut reasons = Vec::new();
                if saved.planner_revision != current.planner_revision {
                    reasons
                        .push("The CAM planner changed since this path was generated.".to_string());
                }
                if saved.model_fingerprint != current.model_fingerprint {
                    reasons.push("Referenced CAD model or sketch geometry changed.".to_string());
                }
                if saved.setup_fingerprint != current.setup_fingerprint {
                    reasons.push("Setup, WCS, stock, or work-offset settings changed.".to_string());
                }
                if saved.operation_fingerprint != current.operation_fingerprint {
                    reasons.push(if saved.order_dependencies.is_none() {
                        "Legacy generation inputs or preceding toolpath order changed; regenerate to verify the new dependency rules."
                    } else {
                        "Operation geometry or cutting/linking settings changed."
                    }.to_string());
                }
                if let (Some(saved), Some(current)) =
                    (&saved.order_dependencies, &current.order_dependencies)
                {
                    if saved.rules_revision != current.rules_revision {
                        reasons.push(
                            "The CAM order-dependency rules changed; regenerate this path."
                                .to_string(),
                        );
                    }
                    if saved.stock_height_fingerprint != current.stock_height_fingerprint {
                        reasons.push("Earlier facing operations used to establish incoming stock height moved, changed, or were suppressed.".to_string());
                    }
                    if saved.predrill_fingerprint != current.predrill_fingerprint {
                        reasons.push("Earlier drilling required by this path's Predrill entry moved, changed, or was suppressed.".to_string());
                    }
                }
                if saved.tool_fingerprint != current.tool_fingerprint {
                    reasons.push("The operation's tool definition changed.".to_string());
                }
                if saved.upstream_fingerprint != current.upstream_fingerprint {
                    reasons.push("An upstream rest-stock setup or tool changed.".to_string());
                }
                statuses.push(CamToolpathStatusDto {
                    setup_id: setup.id,
                    operation_id: operation.id(),
                    state: if reasons.is_empty() {
                        CamToolpathStateDto::Current
                    } else {
                        CamToolpathStateDto::Stale
                    },
                    reasons,
                });
            }
        }
        Ok(statuses)
    }

    pub fn cam_toolpath_safety_warning(
        &self,
        setup_id: u64,
    ) -> Result<Option<String>, SessionError> {
        let mut setup_ids = BTreeSet::from([setup_id]);
        let mut cursor = self.cam.setup(setup_id);
        while let Some(CamSetupDto {
            resolved_stock: CamResolvedStockDto::Rest { source_setup_id },
            ..
        }) = cursor
        {
            if !setup_ids.insert(*source_setup_id) {
                break;
            }
            cursor = self.cam.setup(*source_setup_id);
        }
        let stale = self
            .cam_toolpath_statuses()?
            .into_iter()
            .filter(|status| {
                setup_ids.contains(&status.setup_id) && status.state != CamToolpathStateDto::Current
            })
            .collect::<Vec<_>>();
        if stale.is_empty() {
            return Ok(None);
        }
        let invalid = stale
            .iter()
            .filter(|status| status.state == CamToolpathStateDto::Invalid)
            .flat_map(|status| status.reasons.iter().cloned())
            .collect::<Vec<_>>();
        if !invalid.is_empty() {
            return Ok(Some(format!("SAFETY: Invalid toolpath configuration: {}. Correct the assigned tool or operation, then regenerate before posting NC.", invalid.join("; "))));
        }
        let names = stale
            .iter()
            .filter_map(|status| {
                self.cam
                    .setups
                    .iter()
                    .flat_map(|setup| setup.operations.iter())
                    .find(|operation| operation.id() == status.operation_id)
                    .map(|operation| operation.name())
            })
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(format!(
            "SAFETY: {} enabled toolpath{} out of date ({names}). Regenerate before posting NC.",
            stale.len(),
            if stale.len() == 1 { " is" } else { "s are" },
        )))
    }

    fn ensure_cam_toolpaths_current(&self, setup_id: u64) -> Result<(), SessionError> {
        if let Some(warning) = self.cam_toolpath_safety_warning(setup_id)? {
            return Err(SessionError::Solid(format!(
                "NC posting blocked. {warning} Right-click the setup to regenerate all paths, or regenerate each affected path individually."
            )));
        }
        Ok(())
    }

    /// Resolve persisted height expressions only during an explicit
    /// regeneration transaction. Operations without an expression record are
    /// legacy/manual absolute-Z programs and are intentionally left alone.
    fn resolve_cam_height_expressions(
        &self,
        setup: &mut CamSetupDto,
        only_operation: Option<u64>,
    ) -> Result<(), SessionError> {
        let scene = self.solids.scene();
        let sketches = self.finished_sketches();
        let wanted_bodies = setup.body_ids.iter().copied().collect::<BTreeSet<_>>();
        let mut model_top = f64::NEG_INFINITY;
        let mut model_bottom = f64::INFINITY;
        for body in scene
            .bodies
            .iter()
            .filter(|body| wanted_bodies.contains(&body.id))
        {
            for point in body.mesh.positions.as_chunks::<3>().0 {
                let projected = cam_model_point_to_setup(
                    [
                        f64::from(point[0]),
                        f64::from(point[1]),
                        f64::from(point[2]),
                    ],
                    setup,
                );
                model_top = model_top.max(projected.z);
                model_bottom = model_bottom.min(projected.z);
            }
        }
        if !model_top.is_finite() {
            model_top = setup.stock.max.z;
        }
        if !model_bottom.is_finite() {
            model_bottom = setup.stock.min.z;
        }
        let setup_snapshot = setup.clone();

        for operation in &mut setup.operations {
            if !operation.enabled() || only_operation.is_some_and(|id| operation.id() != id) {
                continue;
            }
            let mut matching_expressions = self
                .cam
                .height_expressions
                .iter()
                .filter(|entry| entry.operation_id == operation.id());
            let Some(expressions) = matching_expressions.next().cloned() else {
                continue;
            };
            let label = operation.name().to_string();
            if matching_expressions.next().is_some() {
                return Err(SessionError::Solid(format!(
                    "Cannot regenerate operation '{label}': duplicate associative height records must be repaired first."
                )));
            }
            expressions
                .validate_for_operation(operation)
                .map_err(|message| {
                    SessionError::Solid(format!(
                        "Cannot regenerate operation '{label}': invalid associative height intent: {message}."
                    ))
                })?;
            let holes: &[CamHoleDto] = match operation {
                CamOperationDto::Drill { holes, .. } | CamOperationDto::Thread { holes, .. } => {
                    holes
                }
                _ => &[],
            };
            let hole_top = holes.iter().map(|hole| hole.top_z).reduce(f64::max);
            let hole_bottom = holes.iter().map(|hole| hole.bottom_z).reduce(f64::min);
            let selection_z = if cam_height_expressions_use_selection(&expressions) {
                Some(cam_selection_reference_z(
                    operation,
                    &setup_snapshot,
                    scene,
                    &sketches,
                    &label,
                )?)
            } else {
                None
            };
            let resolve = |expression: &CamHeightExpressionDto,
                           bottom: Option<f64>,
                           top: Option<f64>,
                           feed: Option<f64>,
                           retract: Option<f64>|
             -> Result<f64, SessionError> {
                let base = match expression.reference {
                    CamHeightReferenceDto::ModelTop => model_top,
                    CamHeightReferenceDto::ModelBottom => model_bottom,
                    CamHeightReferenceDto::StockTop => setup_snapshot.stock.max.z,
                    CamHeightReferenceDto::StockBottom => setup_snapshot.stock.min.z,
                    CamHeightReferenceDto::Origin => 0.0,
                    CamHeightReferenceDto::Geometry => crate::cam_height_geometry::resolve(
                        expression.geometry.as_ref().ok_or_else(|| SessionError::Solid("Height geometry is missing".into()))?,
                        &setup_snapshot, scene, &sketches,
                    ).map_err(SessionError::Solid)?,
                    CamHeightReferenceDto::HoleTop => hole_top.ok_or_else(|| {
                        SessionError::Solid(format!(
                            "Cannot regenerate operation '{label}': its height references picked-hole tops, but no associated hole faces remain. Reselect the holes."
                        ))
                    })?,
                    CamHeightReferenceDto::HoleBottom => hole_bottom.ok_or_else(|| {
                        SessionError::Solid(format!(
                            "Cannot regenerate operation '{label}': its height references picked-hole bottoms, but no associated hole faces remain. Reselect the holes."
                        ))
                    })?,
                    CamHeightReferenceDto::Bottom => bottom.ok_or_else(|| {
                        SessionError::Solid(format!(
                            "Cannot regenerate operation '{label}': a height references Bottom before Bottom is available."
                        ))
                    })?,
                    CamHeightReferenceDto::Top => top.ok_or_else(|| {
                        SessionError::Solid(format!(
                            "Cannot regenerate operation '{label}': a height references Top before Top is available."
                        ))
                    })?,
                    CamHeightReferenceDto::Feed => feed.ok_or_else(|| {
                        SessionError::Solid(format!(
                            "Cannot regenerate operation '{label}': a height references Feed before Feed is available."
                        ))
                    })?,
                    CamHeightReferenceDto::Retract => retract.ok_or_else(|| {
                        SessionError::Solid(format!(
                            "Cannot regenerate operation '{label}': a height references Retract before Retract is available."
                        ))
                    })?,
                    CamHeightReferenceDto::Selection => selection_z.ok_or_else(|| {
                        SessionError::Solid(format!(
                            "Cannot regenerate operation '{label}': its Selection height has no associated sketch plane. Reselect its geometry."
                        ))
                    })?,
                };
                let value = base + expression.offset;
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(SessionError::Solid(format!(
                        "Cannot regenerate operation '{label}': a height expression is not finite."
                    )))
                }
            };

            let needs_bottom = !matches!(operation, CamOperationDto::Chamfer2d { .. });
            let bottom = match &expressions.bottom {
                Some(expression) if needs_bottom => {
                    Some(resolve(expression, None, None, None, None)?)
                }
                None if !needs_bottom => None,
                Some(_) => {
                    return Err(SessionError::Solid(format!(
                        "Cannot regenerate operation '{label}': this operation does not take a Bottom height."
                    )))
                }
                None => {
                    return Err(SessionError::Solid(format!(
                        "Cannot regenerate operation '{label}': its associative Bottom height is missing."
                    )))
                }
            };
            let top = match operation {
                CamOperationDto::Chamfer2d {
                    modeled_chamfer: Some(_),
                    ..
                } => operation
                    .chamfer_chains()
                    .iter()
                    .map(|c| c.top_z)
                    .fold(f64::NEG_INFINITY, f64::max),
                _ => resolve(&expressions.top, bottom, None, None, None)?,
            };
            let feed = resolve(&expressions.feed, bottom, Some(top), None, None)?;
            let retract = resolve(&expressions.retract, bottom, Some(top), Some(feed), None)?;
            let clearance = resolve(
                &expressions.clearance,
                bottom,
                Some(top),
                Some(feed),
                Some(retract),
            )?;
            cam_apply_resolved_heights(operation, bottom, top, feed, retract, clearance)?;
            // A picked hole face only spans the cylinder it bounds. Unless a
            // height references the holes themselves, the operation's
            // Top/Bottom is the operator's depth for every hole (e.g. Bottom
            // = model bottom drills through, not to the end of one face).
            if let CamOperationDto::Drill { holes, .. } | CamOperationDto::Thread { holes, .. } =
                operation
            {
                let hole_bottom = matches!(
                    expressions.bottom.as_ref().map(|e| e.reference),
                    Some(CamHeightReferenceDto::HoleBottom)
                );
                let hole_top = matches!(expressions.top.reference, CamHeightReferenceDto::HoleTop);
                for hole in holes {
                    if let (false, Some(bottom)) = (hole_bottom, bottom) {
                        hole.bottom_z = bottom;
                    }
                    if !hole_top {
                        hole.top_z = top;
                    }
                }
            }
        }
        Ok(())
    }

    fn resolve_cam_adaptive_geometry(
        &self,
        setup: &mut CamSetupDto,
        only_operation: Option<u64>,
    ) -> Result<(), SessionError> {
        if !setup.operations.iter().any(|op| {
            op.enabled()
                && only_operation.is_none_or(|id| op.id() == id)
                && matches!(
                    op,
                    CamOperationDto::Adaptive3d { .. } | CamOperationDto::Flat3d { .. }
                )
        }) {
            return Ok(());
        }
        if setup.body_ids.is_empty() {
            return Err(SessionError::Solid(
                "3D milling (High Speed Roughing, Flat) needs target bodies selected in the setup."
                    .into(),
            ));
        }
        let scene = self.solids.scene();
        let mesh_for = |id: BodyId| -> Result<CamStockMeshDto, SessionError> {
            let body = scene
                .bodies
                .iter()
                .find(|body| body.id == id)
                .ok_or_else(|| {
                    SessionError::Solid(format!(
                        "3D milling target body {} no longer exists.",
                        id.0
                    ))
                })?;
            Ok(CamStockMeshDto {
                positions: body.mesh.positions.iter().map(|&v| f64::from(v)).collect(),
                indices: body.mesh.indices.clone(),
            })
        };
        let mut stock_source = &*setup;
        let mut seen = BTreeSet::new();
        while let CamResolvedStockDto::Rest { source_setup_id } = stock_source.resolved_stock {
            if !seen.insert(source_setup_id) {
                return Err(SessionError::Solid("Rest-stock setup cycle".into()));
            }
            stock_source = self
                .cam
                .setup(source_setup_id)
                .ok_or_else(|| SessionError::Solid("Missing rest-stock source setup".into()))?;
        }
        let geometry = CamAdaptiveGeometryDto {
            targets: setup
                .body_ids
                .iter()
                .copied()
                .map(mesh_for)
                .collect::<Result<Vec<_>, _>>()?,
            stock: if let CamResolvedStockDto::ModelBody { body_id } = &stock_source.resolved_stock
            {
                Some(mesh_for(BodyId(*body_id))?)
            } else {
                None
            },
        };
        for operation in &mut setup.operations {
            if !operation.enabled() || only_operation.is_some_and(|id| operation.id() != id) {
                continue;
            }
            if let CamOperationDto::Adaptive3d {
                geometry: snapshot, ..
            }
            | CamOperationDto::Flat3d {
                geometry: snapshot, ..
            } = operation
            {
                *snapshot = Some(geometry.clone());
            }
        }
        Ok(())
    }

    /// Re-resolve every persisted model/sketch association before a
    /// regeneration is allowed to earn a fresh dependency stamp. Raw manual
    /// coordinates remain raw; referenced chains and cylindrical faces may
    /// never silently fall back to their last baked coordinates.
    fn resolve_cam_associative_geometry(
        &self,
        setup: &mut CamSetupDto,
        only_operation: Option<u64>,
    ) -> Result<(), SessionError> {
        let scene = self.solids.scene();
        let sketches = self.finished_sketches();
        let setup_snapshot = setup.clone();
        for operation in &mut setup.operations {
            if !operation.enabled() || only_operation.is_some_and(|id| operation.id() != id) {
                continue;
            }
            match operation {
                CamOperationDto::Contour2d {
                    name,
                    path,
                    closed,
                    chain_ref: Some(reference),
                    ..
                } => {
                    let resolved = resolve_cam_chain(
                        reference.source,
                        &reference.keys,
                        reference.reversed,
                        &setup_snapshot,
                        scene,
                        &sketches,
                        false,
                    )
                    .map_err(|message| {
                        SessionError::Solid(format!(
                            "Cannot regenerate contour '{name}': {message} Reselect its geometry."
                        ))
                    })?;
                    *path = resolved.0;
                    *closed = resolved.1;
                }
                CamOperationDto::Pocket2d {
                    name,
                    outline,
                    chain_ref: Some(reference),
                    ..
                } => {
                    let (resolved, closed) = resolve_cam_chain(
                        reference.source,
                        &reference.keys,
                        reference.reversed,
                        &setup_snapshot,
                        scene,
                        &sketches,
                        true,
                    )
                    .map_err(|message| {
                        SessionError::Solid(format!(
                            "Cannot regenerate operation '{name}': {message} Reselect its geometry."
                        ))
                    })?;
                    if !closed {
                        return Err(SessionError::Solid(format!(
                            "Cannot regenerate operation '{name}': its referenced entities no longer form a closed loop. Reselect its geometry."
                        )));
                    }
                    *outline = resolved;
                }
                CamOperationDto::Chamfer2d { .. } => {
                    let name = operation.name().to_owned();
                    let mut chains = operation.chamfer_chains();
                    for (i, chain) in chains.iter_mut().enumerate() {
                        let Some(reference) = &chain.chain_ref else {
                            continue;
                        };
                        let failure = |e| {
                            SessionError::Solid(format!(
                                "Cannot regenerate chamfer '{name}', chain {}: {e}",
                                i + 1
                            ))
                        };
                        if let Some(modeled) = &chain.modeled_chamfer {
                            let resolved =
                                crate::cam_chamfer::resolve(scene, &setup_snapshot, reference)
                                    .map_err(failure)?;
                            chain.path = resolved.path;
                            chain.closed = resolved.closed;
                            chain.top_z = resolved.top_z;
                            chain.chamfer_width = resolved.width + modeled.additional_width;
                            chain.wall_side = resolved.wall_side;
                        } else {
                            let resolved = resolve_cam_chain(
                                reference.source,
                                &reference.keys,
                                reference.reversed,
                                &setup_snapshot,
                                scene,
                                &sketches,
                                true,
                            )
                            .map_err(failure)?;
                            chain.path = resolved.0;
                            chain.closed = resolved.1;
                        }
                    }
                    operation.set_chamfer_chains(chains);
                }
                CamOperationDto::Drill { name, holes, .. }
                | CamOperationDto::Thread { name, holes, .. } => {
                    for hole in holes {
                        let Some(reference) = hole.face_key.clone() else {
                            continue;
                        };
                        resolve_cam_hole(&reference, hole, &setup_snapshot, scene).map_err(
                            |message| {
                                SessionError::Solid(format!(
                                    "Cannot regenerate operation '{name}': {message} Reselect the hole face."
                                ))
                            },
                        )?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn cam_regenerate_operation(
        &mut self,
        operation_id: u64,
    ) -> Result<CamDocumentDto, SessionError> {
        let (setup_id, operation) = self
            .cam
            .setups
            .iter()
            .find_map(|setup| {
                setup
                    .operations
                    .iter()
                    .find(|operation| operation.id() == operation_id)
                    .map(|operation| (setup.id, operation.clone()))
            })
            .ok_or_else(|| SessionError::Solid("CAM operation does not exist".to_string()))?;
        if !operation.enabled() {
            return Err(SessionError::Solid(
                "Resume the operation before regenerating its toolpath.".to_string(),
            ));
        }
        let setup = self
            .cam
            .setups
            .iter()
            .find(|setup| setup.id == setup_id)
            .expect("operation owner was found above");
        self.ensure_cam_model_references_exist(setup)?;
        if let CamResolvedStockDto::Rest { source_setup_id } = setup.resolved_stock {
            if let Some(warning) = self.cam_toolpath_safety_warning(source_setup_id)? {
                return Err(SessionError::Solid(format!(
                    "Regenerate the source setup before calculating remaining stock. {warning}"
                )));
            }
        }
        let mut resolved_setup = setup.clone();
        self.resolve_cam_associative_geometry(&mut resolved_setup, Some(operation_id))?;
        self.resolve_cam_height_expressions(&mut resolved_setup, Some(operation_id))?;
        self.resolve_cam_adaptive_geometry(&mut resolved_setup, Some(operation_id))?;
        let operation = resolved_setup
            .operations
            .iter()
            .find(|op| op.id() == operation_id)
            .expect("operation owner was found above")
            .clone();

        let mut isolated = self.cam.clone();
        if let Some(setup) = isolated
            .setups
            .iter_mut()
            .find(|setup| setup.id == setup_id)
        {
            *setup = resolved_setup.clone();
            let through = setup
                .operations
                .iter()
                .position(|candidate| candidate.id() == operation_id)
                .expect("validated operation")
                + 1;
            setup.operations.truncate(through);
        }
        let kept = isolated
            .setups
            .iter()
            .flat_map(|s| s.operations.iter().map(CamOperationDto::id))
            .collect::<BTreeSet<_>>();
        isolated
            .height_expressions
            .retain(|h| kept.contains(&h.operation_id));
        isolated.linking.retain(|l| kept.contains(&l.operation_id));
        plan_setup(&isolated, setup_id).map_err(|error| SessionError::Solid(error.to_string()))?;

        let dependencies = self.cam_setup_dependency_fingerprints(&resolved_setup)?;
        let generation = self.current_cam_generation(&resolved_setup, &operation, &dependencies)?;
        *self
            .cam
            .setups
            .iter_mut()
            .find(|setup| setup.id == setup_id)
            .expect("validated owner") = resolved_setup;
        self.cam
            .toolpath_generations
            .retain(|saved| saved.operation_id != operation_id);
        self.cam.toolpath_generations.push(generation);
        self.cam
            .toolpath_generations
            .sort_by_key(|saved| saved.operation_id);
        Ok(self.cam.clone())
    }

    pub fn cam_regenerate_setup(&mut self, setup_id: u64) -> Result<CamDocumentDto, SessionError> {
        let setup = self
            .cam
            .setups
            .iter()
            .find(|setup| setup.id == setup_id)
            .ok_or_else(|| SessionError::Solid("CAM setup does not exist".to_string()))?;
        self.ensure_cam_model_references_exist(setup)?;
        if let CamResolvedStockDto::Rest { source_setup_id } = setup.resolved_stock {
            if let Some(warning) = self.cam_toolpath_safety_warning(source_setup_id)? {
                return Err(SessionError::Solid(format!(
                    "Regenerate the source setup before calculating remaining stock. {warning}"
                )));
            }
        }
        let mut resolved_setup = setup.clone();
        self.resolve_cam_associative_geometry(&mut resolved_setup, None)?;
        self.resolve_cam_height_expressions(&mut resolved_setup, None)?;
        self.resolve_cam_adaptive_geometry(&mut resolved_setup, None)?;
        let mut candidate = self.cam.clone();
        *candidate
            .setups
            .iter_mut()
            .find(|s| s.id == setup_id)
            .expect("validated setup") = resolved_setup.clone();
        plan_setup(&candidate, setup_id).map_err(|error| SessionError::Solid(error.to_string()))?;
        let setup = &resolved_setup;
        let dependencies = self.cam_setup_dependency_fingerprints(setup)?;
        let operation_ids = setup
            .operations
            .iter()
            .map(CamOperationDto::id)
            .collect::<BTreeSet<_>>();
        let generations = setup
            .operations
            .iter()
            .filter(|operation| operation.enabled())
            .map(|operation| self.current_cam_generation(setup, operation, &dependencies))
            .collect::<Result<Vec<_>, _>>()?;
        *self
            .cam
            .setups
            .iter_mut()
            .find(|s| s.id == setup_id)
            .expect("validated setup") = resolved_setup;
        self.cam
            .toolpath_generations
            .retain(|saved| !operation_ids.contains(&saved.operation_id));
        self.cam.toolpath_generations.extend(generations);
        self.cam
            .toolpath_generations
            .sort_by_key(|saved| saved.operation_id);
        Ok(self.cam.clone())
    }

    pub fn cam_plan(&self, setup_id: u64) -> Result<CamProgramDto, SessionError> {
        let mut program = plan_setup(&self.cam, setup_id)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        if let Some(warning) = self.cam_toolpath_safety_warning(setup_id)? {
            program.warnings.insert(0, warning);
        }
        Ok(program)
    }

    pub fn cam_plan_through(
        &self,
        setup_id: u64,
        operation_id: u64,
    ) -> Result<CamProgramDto, SessionError> {
        let mut program = limo_cad_cam::plan_setup_through(&self.cam, setup_id, operation_id)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        if let Some(warning) = self.cam_toolpath_safety_warning(setup_id)? {
            program.warnings.insert(0, warning);
        }
        Ok(program)
    }

    pub fn cam_post(&self, request: CamPostRequestDto) -> Result<CamPostResultDto, SessionError> {
        self.ensure_cam_toolpaths_current(request.setup_id)?;
        let mut result = post_setup(&self.cam, &request)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        let verification = self.cam_post_verification(request.setup_id)?;
        result.warnings.extend(verification);
        Ok(result)
    }

    /// Fresh generation is not geometric verification. Recheck current
    /// incoming stock, project tool offsets, WCS and target meshes before
    /// either export route. The kernel's content-keyed bounded cache reuses
    /// evidence only for identical inputs; no persisted "passed" bit can
    /// survive an edit or a different controller radius assumption.
    fn cam_post_verification(&self, setup_id: u64) -> Result<Vec<String>, SessionError> {
        let setup = self
            .cam
            .setup(setup_id)
            .ok_or_else(|| SessionError::Solid("CAM setup no longer exists".into()))?;
        let scene = self.solids.scene();
        let mesh_for = |id: BodyId| -> Result<CamStockMeshDto, SessionError> {
            let body = scene
                .bodies
                .iter()
                .find(|body| body.id == id)
                .ok_or_else(|| {
                    SessionError::Solid(format!(
                        "Cannot verify CAM export: body {} no longer exists",
                        id.0
                    ))
                })?;
            Ok(CamStockMeshDto {
                positions: body.mesh.positions.iter().map(|&v| f64::from(v)).collect(),
                indices: body.mesh.indices.clone(),
            })
        };
        let target = if setup.body_ids.is_empty() {
            None
        } else {
            Some(CamSimulationTargetDto {
                cache_key: None,
                tolerance_mm: 0.1,
                meshes: setup
                    .body_ids
                    .iter()
                    .copied()
                    .map(mesh_for)
                    .collect::<Result<Vec<_>, _>>()?,
            })
        };
        let mut source = setup;
        for _ in 0..self.cam.setups.len() {
            if let CamResolvedStockDto::Rest { source_setup_id } = source.resolved_stock {
                source = self.cam.setup(source_setup_id).ok_or_else(|| {
                    SessionError::Solid(
                        "Cannot verify CAM export: missing rest-stock source".into(),
                    )
                })?;
            } else {
                break;
            }
        }
        let stock_mesh = if let CamResolvedStockDto::ModelBody { body_id } = source.resolved_stock {
            Some(mesh_for(BodyId(body_id))?)
        } else {
            None
        };
        let checked_target = target.is_some();
        let result = simulate_setup(
            &self.cam,
            &CamSimulationRequestDto {
                setup_id,
                voxel_size: None,
                max_voxels: None,
                stock_mesh,
                target,
                through_operation_id: None,
                completed_steps: None,
                playback_time_seconds: None,
            },
        )
        .map_err(|error| SessionError::Solid(format!("CAM export verification failed: {error}")))?;
        let program = plan_setup(&self.cam, setup_id)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        let axial_stock_removal: std::collections::HashSet<usize> = result
            .steps
            .iter()
            .filter(|step| {
                step.removed_voxels > 0
                    && step.from.zip(step.to).is_some_and(|(from, to)| {
                        (from.x - to.x).abs() < 1e-8
                            && (from.y - to.y).abs() < 1e-8
                            && to.z < from.z - 1e-8
                    })
            })
            .map(|step| step.command_index)
            .collect();
        let mut thread_section = false;
        for (index, command) in program.commands.iter().enumerate() {
            if let limo_cad_cam::CamCommandDto::SectionStart { operation_id, .. } = command {
                thread_section = setup.operations.iter().any(|op| {
                    op.id() == *operation_id && matches!(op, CamOperationDto::Thread { .. })
                });
            }
            if thread_section
                && axial_stock_removal.contains(&index)
                && matches!(command, limo_cad_cam::CamCommandDto::Linear { .. })
            {
                return Err(SessionError::Solid(format!("CAM export blocked: thread-tool entry at motion {} removes incoming stock. Generate the upstream bore and verify its full-diameter depth, including the drill point, before thread milling", index + 1)));
            }
        }
        if let Some(issue) = result.collisions.first() {
            return Err(SessionError::Solid(format!("CAM export blocked by current stock/target verification at motion {}: {}. Inspect CAM Sim and regenerate corrected paths", issue.command_index + 1, issue.message)));
        }
        if result
            .comparison
            .as_ref()
            .is_some_and(|comparison| comparison.initial_shortfall_voxels > 0)
        {
            return Err(SessionError::Solid("CAM export blocked: incoming stock is already missing protected target volume; inspect stock/WCS and earlier rest-source operations".into()));
        }
        if let Some(warning) = result
            .warnings
            .iter()
            .find(|warning| warning.contains("unpaired surface crossings"))
        {
            return Err(SessionError::Solid(format!(
                "CAM export could not verify mesh closure: {warning}"
            )));
        }
        let mut warnings = result.warnings;
        warnings.push(if checked_target {
            "Current stock and target checked at the simulation's disclosed voxel tolerance using the project cutter diameter. Match the controller's radius offset; fixtures, holders and machine motion remain unverified.".into()
        } else {
            "Stock-contact check completed, but no target bodies are selected: part-gouge clearance is UNVERIFIED. Select setup target bodies for geometric comparison. Fixtures, holders and machine motion remain unverified.".into()
        });
        Ok(warnings)
    }

    pub fn cam_analyze_nbpost(
        &self,
        request: NbPostAnalysisRequestDto,
    ) -> Result<NbPostAnalysisDto, SessionError> {
        analyze_nbpost(&request).map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn cam_simulate(
        &self,
        request: CamSimulationRequestDto,
    ) -> Result<CamSimulationResultDto, SessionError> {
        let mut result = simulate_setup(&self.cam, &request)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        if let Some(warning) = self.cam_toolpath_safety_warning(request.setup_id)? {
            result.warnings.insert(0, warning);
        }
        Ok(result)
    }

    pub fn cam_simulate_gcode(
        &self,
        request: CamGcodeSimulationRequestDto,
    ) -> Result<CamSimulationResultDto, SessionError> {
        simulate_gcode(&self.cam, &request).map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn cam_post_events(&self, setup_id: u64) -> Result<PostEventStreamDto, SessionError> {
        self.ensure_cam_toolpaths_current(setup_id)?;
        let verification = self.cam_post_verification(setup_id)?;
        let mut program = self.cam_plan(setup_id)?;
        program.warnings.extend(verification);
        Ok(post_event_stream(&self.cam, &program))
    }

    pub fn set_body_appearance(
        &mut self,
        appearance: BodyAppearance,
    ) -> Result<Vec<BodyAppearance>, SessionError> {
        if appearance.body_id.0 == 0 {
            return Err(SessionError::Solid(
                "body appearance requires a non-zero body id".to_string(),
            ));
        }
        if let Some(material) = &appearance.material {
            material.validate().map_err(SessionError::Solid)?;
        }
        let material_name = appearance.material_name.trim();
        let material_name = if material_name.is_empty() {
            DEFAULT_MATERIAL_NAME.to_string()
        } else {
            material_name.to_string()
        };
        let next = BodyAppearance {
            body_id: appearance.body_id,
            color: appearance.color,
            material_name,
            filament_type: appearance.filament_type.trim().to_string(),
            brand: appearance.brand.trim().to_string(),
            color_name: appearance.color_name.trim().to_string(),
            filament_id: appearance
                .filament_id
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty()),
            preset_id: appearance
                .preset_id
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty()),
            density_g_cm3: appearance
                .density_g_cm3
                .filter(|d| d.is_finite() && *d > 0.0),
            diameter_mm: if appearance.diameter_mm.is_finite() && appearance.diameter_mm > 0.0 {
                appearance.diameter_mm
            } else {
                limo_cad_core::DEFAULT_FILAMENT_DIAMETER_MM
            },
            material: appearance.material,
        };
        if let Some(existing) = self
            .body_appearances
            .iter_mut()
            .find(|entry| entry.body_id == next.body_id)
        {
            *existing = next;
        } else {
            self.body_appearances.push(next);
        }
        self.body_appearances.sort_by_key(|entry| entry.body_id.0);
        Ok(self.body_appearances.clone())
    }

    fn retained_presentation_body_ids(&self) -> BTreeSet<limo_cad_core::BodyId> {
        let mut retained = self.solids.retained_body_ids();
        retained.extend(self.solids.scene().bodies.iter().map(|body| body.id));
        retained
    }

    fn scrubbed_body_appearances(&self) -> Vec<BodyAppearance> {
        let retained = self.retained_presentation_body_ids();
        let mut kept: Vec<_> = self
            .body_appearances
            .iter()
            .filter(|entry| retained.contains(&entry.body_id))
            .cloned()
            .collect();
        kept.sort_by_key(|entry| entry.body_id.0);
        kept
    }

    fn scrub_body_appearances(&mut self) {
        self.body_appearances = self.scrubbed_body_appearances();
    }

    fn scrubbed_project_visibility(&self) -> ProjectVisibilityDto {
        let retained_bodies = self.retained_presentation_body_ids();
        let live_datums = self
            .datum_planes
            .iter()
            .map(|plane| plane.datum_id.0)
            .collect::<BTreeSet<_>>();
        let live_sketches = self
            .finished
            .iter()
            .map(|sketch| sketch.session.name().to_string())
            .collect::<BTreeSet<_>>();

        let mut hidden_body_ids = self
            .project_visibility
            .hidden_body_ids
            .iter()
            .copied()
            .filter(|id| retained_bodies.contains(&limo_cad_core::BodyId(*id)))
            .collect::<Vec<_>>();
        hidden_body_ids.sort_unstable();
        hidden_body_ids.dedup();

        let mut hidden_datum_plane_ids = self
            .project_visibility
            .hidden_datum_plane_ids
            .iter()
            .copied()
            .filter(|id| live_datums.contains(id))
            .collect::<Vec<_>>();
        hidden_datum_plane_ids.sort_unstable();
        hidden_datum_plane_ids.dedup();

        let mut hidden_sketch_names = self
            .project_visibility
            .hidden_sketch_names
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty() && live_sketches.contains(*name))
            .map(str::to_string)
            .collect::<Vec<_>>();
        hidden_sketch_names.sort();
        hidden_sketch_names.dedup();

        ProjectVisibilityDto {
            hidden_body_ids,
            hidden_datum_plane_ids,
            hidden_sketch_names,
        }
    }

    fn scrub_project_visibility(&mut self) {
        self.project_visibility = self.scrubbed_project_visibility();
    }

    fn scrubbed_named_views(&self) -> Vec<NamedViewConfigurationDto> {
        let retained = self.retained_presentation_body_ids();
        self.named_views
            .iter()
            .map(|view| {
                let mut visible_body_ids = view
                    .visible_body_ids
                    .iter()
                    .copied()
                    .filter(|id| retained.contains(&limo_cad_core::BodyId(*id)))
                    .collect::<Vec<_>>();
                visible_body_ids.sort_unstable();
                visible_body_ids.dedup();
                let mut part_offsets = view
                    .part_offsets
                    .iter()
                    .filter(|offset| retained.contains(&limo_cad_core::BodyId(offset.body_id)))
                    .cloned()
                    .collect::<Vec<_>>();
                part_offsets.sort_by_key(|offset| offset.body_id);
                NamedViewConfigurationDto {
                    id: view.id.clone(),
                    name: view.name.clone(),
                    camera: view.camera.clone(),
                    visible_body_ids,
                    part_offsets,
                    occurrence_offsets: view
                        .occurrence_offsets
                        .iter()
                        .filter(|offset| {
                            self.assembly
                                .component_structure
                                .occurrences
                                .iter()
                                .any(|o| o.id == offset.occurrence_id)
                        })
                        .cloned()
                        .collect(),
                    print_layout: view.print_layout,
                    print_bed: view.print_bed.clone(),
                }
            })
            .collect()
    }

    fn scrub_named_views(&mut self) {
        self.named_views = self.scrubbed_named_views();
        if self
            .active_named_view
            .as_ref()
            .is_some_and(|name| !self.named_views.iter().any(|view| &view.name == name))
        {
            self.active_named_view = None;
        }
    }

    fn sync_named_view_browser(&mut self) {
        let names = self
            .named_views
            .iter()
            .map(|view| view.name.clone())
            .collect::<Vec<_>>();
        self.document.set_named_view_children(&names);
    }

    pub fn extrude_definitions(&self) -> Vec<ExtrudeDefinitionDto> {
        self.solids.definitions().to_vec()
    }

    pub fn revolve_definitions(&self) -> Vec<RevolveDefinitionDto> {
        self.solids.revolve_definitions().to_vec()
    }

    pub fn sweep_definitions(&self) -> Vec<SweepDefinitionDto> {
        self.solids.sweep_definitions().to_vec()
    }

    pub fn loft_definitions(&self) -> Vec<LoftDefinitionDto> {
        self.solids.loft_definitions().to_vec()
    }

    pub fn rib_definitions(&self) -> Vec<RibDefinitionDto> {
        self.solids.rib_definitions().to_vec()
    }

    pub fn fillet_definitions(&self) -> Vec<SolidFilletDefinitionDto> {
        self.solids.fillet_definitions().to_vec()
    }

    pub fn chamfer_definitions(&self) -> Vec<SolidChamferDefinitionDto> {
        self.solids.chamfer_definitions().to_vec()
    }

    pub fn hole_definitions(&self) -> Vec<HoleDefinitionDto> {
        self.solids.hole_definitions().to_vec()
    }

    pub fn datum_plane_definitions(&self) -> Vec<DatumPlaneDefinitionDto> {
        self.datum_planes.clone()
    }

    pub fn body_feature_definitions(&self) -> Vec<BodyFeatureDefinitionDto> {
        self.solids.body_feature_definitions().to_vec()
    }

    pub fn create_datum_plane(
        &mut self,
        mut request: DatumPlaneRequest,
    ) -> Result<DatumPlaneUpdateDto, SessionError> {
        self.ensure_no_active_sketch("creating a construction plane")?;
        let active = self.active_feature_ids();
        let basis = resolve_datum_source(
            &self.solids,
            &self.datum_planes,
            &active,
            &mut request.source,
        )?;
        let next_number = max_feature_number(&self.document, "Plane") + 1;
        let name = request
            .name
            .as_deref()
            .map(str::trim)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Plane{next_number}"));
        if name.is_empty()
            || name.chars().any(char::is_control)
            || self
                .document
                .features()
                .features
                .iter()
                .any(|f| f.name == name)
        {
            return Err(SessionError::Solid(
                "Construction plane name must be non-empty, printable, and unique".into(),
            ));
        }
        let feature_id = self.document.alloc_feature_id();
        let datum_id = FaceId(self.next_datum_id);
        self.next_datum_id += 1;
        self.datum_planes.push(DatumPlaneDefinitionDto {
            feature_id,
            name: name.clone(),
            datum_id,
            source: request.source,
            basis,
        });
        self.document.add_construction_plane_node(datum_id.0, &name);
        self.document
            .features_mut()
            .insert_at_rollback(Feature::new(
                feature_id,
                name,
                FeatureKind::ConstructionPlane,
            ));
        Ok(DatumPlaneUpdateDto {
            document: self.document_dto(),
            planes: self.datum_planes.clone(),
        })
    }

    pub fn edit_datum_plane(
        &mut self,
        mut request: EditDatumPlaneRequest,
    ) -> Result<DatumPlaneUpdateDto, SessionError> {
        self.ensure_no_active_sketch("editing a construction plane")?;
        let active = self.active_feature_ids();
        let definition_index = self
            .datum_planes
            .iter()
            .position(|definition| definition.feature_id == request.feature_id)
            .ok_or_else(|| {
                SessionError::Solid(format!(
                    "construction plane feature {} was not found",
                    request.feature_id.0
                ))
            })?;

        let feature_order = self
            .document
            .features()
            .features
            .iter()
            .enumerate()
            .map(|(index, feature)| (feature.id, index))
            .collect::<BTreeMap<_, _>>();
        let current_position = feature_order
            .get(&request.feature_id)
            .copied()
            .unwrap_or(usize::MAX);
        let prior_planes = self
            .datum_planes
            .iter()
            .filter(|plane| {
                feature_order
                    .get(&plane.feature_id)
                    .is_some_and(|position| *position < current_position)
            })
            .cloned()
            .collect::<Vec<_>>();
        let basis = resolve_datum_source(
            &self.solids,
            &prior_planes,
            &active,
            &mut request.plane.source,
        )?;
        let definition = &mut self.datum_planes[definition_index];
        definition.source = request.plane.source;
        definition.basis = basis;
        let errors = self.refresh_datum_planes(&active);
        for feature in &mut self.document.features_mut().features {
            if feature.kind == FeatureKind::ConstructionPlane {
                feature.status = FeatureStatus::Ok;
            }
        }
        for (feature_id, message) in errors {
            self.document
                .set_feature_status(feature_id, FeatureStatus::Error { message });
        }
        Ok(DatumPlaneUpdateDto {
            document: self.document_dto(),
            planes: self.datum_planes.clone(),
        })
    }

    pub fn prepare_body_feature(
        &mut self,
        request: BodyFeatureRequestDto,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("creating a body operation")?;
        let request = self.hydrate_body_feature_plane(request)?;
        let (kind, prefix) = body_feature_kind(&request);
        let feature_id = self.document.alloc_feature_id();
        let next_number = max_feature_number(&self.document, prefix) + 1;
        let name = format!("{prefix}{next_number}");
        let feature = Feature::new(feature_id, name.clone(), kind);
        self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add_body_feature(feature_id, &name, request, catalog, active)
        })
    }

    /// Prepare a new solid operation at the current build cursor rather than
    /// unconditionally appending it to the end of history. The solid planner
    /// needs the proposed order before it creates its pending transaction, so
    /// update both order models transactionally and restore the prior order if
    /// request validation fails.
    fn prepare_new_solid_feature<F>(
        &mut self,
        feature: Feature,
        prepare: F,
    ) -> Result<RecomputePlanDto, SessionError>
    where
        F: FnOnce(
            &mut SolidDocument,
            &[ProfileCatalogItemDto],
            &BTreeSet<FeatureId>,
        ) -> Result<RecomputePlanDto, limo_cad_solid::SolidError>,
    {
        let insertion_index = self
            .document
            .features()
            .rollback_index
            .min(self.document.features().features.len());
        let previous_order = self
            .document
            .features()
            .features
            .iter()
            .map(|existing| existing.id)
            .collect::<Vec<_>>();
        let mut proposed_order = previous_order.clone();
        proposed_order.insert(insertion_index, feature.id);
        self.solids
            .set_feature_order(&proposed_order)
            .map_err(|error| SessionError::Solid(error.to_string()))?;

        let mut active = self.active_feature_ids_at(insertion_index);
        active.insert(feature.id);
        let catalog = self.profile_catalog();
        match prepare(&mut self.solids, &catalog, &active) {
            Ok(plan) => {
                self.document.features_mut().insert_at_rollback(feature);
                Ok(plan)
            }
            Err(error) => {
                let _ = self.solids.set_feature_order(&previous_order);
                Err(SessionError::Solid(error.to_string()))
            }
        }
    }

    pub fn prepare_edit_body_feature(
        &mut self,
        request: EditBodyFeatureRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("editing a body operation")?;
        self.validate_active_sketch_references()?;
        let feature = self
            .document
            .features()
            .features
            .iter()
            .find(|feature| feature.id == request.feature_id)
            .ok_or_else(|| {
                SessionError::Solid(format!("feature {} was not found", request.feature_id.0))
            })?;
        let hydrated = self.hydrate_body_feature_plane(request.feature)?;
        let (kind, _) = body_feature_kind(&hydrated);
        if feature.kind != kind {
            return Err(SessionError::Solid(
                "a body feature cannot be edited into a different operation type".to_string(),
            ));
        }
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit_body_feature(
                request.feature_id,
                hydrated,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    fn hydrate_body_feature_plane(
        &self,
        request: BodyFeatureRequestDto,
    ) -> Result<BodyFeatureRequestDto, SessionError> {
        Ok(match request {
            BodyFeatureRequestDto::Mirror(mut mirror) => {
                mirror.plane_basis = Some(self.resolve_plane_basis(mirror.plane)?);
                BodyFeatureRequestDto::Mirror(mirror)
            }
            BodyFeatureRequestDto::SplitBody(mut split) => {
                split.plane_basis = Some(self.resolve_plane_basis(split.plane)?);
                BodyFeatureRequestDto::SplitBody(split)
            }
            other => other,
        })
    }

    fn resolve_plane_basis(&self, reference: PlaneRef) -> Result<PlaneBasis, SessionError> {
        match reference {
            PlaneRef::OriginPlane { .. } => reference
                .origin_basis()
                .map_err(|_| SessionError::UnsupportedPlane),
            PlaneRef::PlanarFace { face_id } => self.solids.face_basis(face_id).ok_or_else(|| {
                SessionError::BrokenReference(format!(
                    "face {} no longer exists or is not planar",
                    face_id.0
                ))
            }),
            PlaneRef::DatumPlane { datum_id } => self
                .resolve_datum_basis(datum_id, &self.active_feature_ids())
                .ok_or_else(|| {
                    SessionError::BrokenReference(format!(
                        "construction plane {} is missing or rolled back",
                        datum_id.0
                    ))
                }),
        }
    }

    fn resolve_datum_basis(
        &self,
        datum_id: FaceId,
        active: &BTreeSet<FeatureId>,
    ) -> Option<PlaneBasis> {
        self.datum_planes
            .iter()
            .find(|plane| plane.datum_id == datum_id && active.contains(&plane.feature_id))
            .map(|plane| plane.basis)
    }

    pub fn prepare_extrude(
        &mut self,
        request: ExtrudeRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before extruding".to_string(),
            ));
        }
        let feature_id = self.document.alloc_feature_id();
        let next_number = self.extrude_count + 1;
        let name = format!("Extrude{next_number}");
        let feature = Feature::new(feature_id, name.clone(), FeatureKind::Extrude);
        let plan = self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add(feature_id, &name, request, catalog, active)
        })?;
        self.extrude_count = next_number;
        Ok(plan)
    }

    pub fn prepare_edit_extrude(
        &mut self,
        request: EditExtrudeRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before editing an Extrude".to_string(),
            ));
        }
        self.validate_active_sketch_references()?;
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit(
                request.feature_id,
                request.extrude,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_revolve(
        &mut self,
        request: RevolveRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before revolving".to_string(),
            ));
        }
        let feature_id = self.document.alloc_feature_id();
        let next_number = self.revolve_count + 1;
        let name = format!("Revolve{next_number}");
        let feature = Feature::new(feature_id, name.clone(), FeatureKind::Revolve);
        let plan = self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add_revolve(feature_id, &name, request, catalog, active)
        })?;
        self.revolve_count = next_number;
        Ok(plan)
    }

    pub fn prepare_edit_revolve(
        &mut self,
        request: EditRevolveRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before editing a Revolve".to_string(),
            ));
        }
        self.validate_active_sketch_references()?;
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit_revolve(
                request.feature_id,
                request.revolve,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_sweep(
        &mut self,
        request: SweepRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before sweeping".to_string(),
            ));
        }
        let feature_id = self.document.alloc_feature_id();
        let next_number = self.sweep_count + 1;
        let name = format!("Sweep{next_number}");
        let feature = Feature::new(feature_id, name.clone(), FeatureKind::Sweep);
        let plan = self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add_sweep(feature_id, &name, request, catalog, active)
        })?;
        self.sweep_count = next_number;
        Ok(plan)
    }

    pub fn prepare_edit_sweep(
        &mut self,
        request: EditSweepRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before editing a Sweep".to_string(),
            ));
        }
        self.validate_active_sketch_references()?;
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit_sweep(
                request.feature_id,
                request.sweep,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_loft(&mut self, request: LoftRequest) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before lofting".to_string(),
            ));
        }
        let feature_id = self.document.alloc_feature_id();
        let next_number = self.loft_count + 1;
        let name = format!("Loft{next_number}");
        let feature = Feature::new(feature_id, name.clone(), FeatureKind::Loft);
        let plan = self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add_loft(feature_id, &name, request, catalog, active)
        })?;
        self.loft_count = next_number;
        Ok(plan)
    }

    pub fn prepare_edit_loft(
        &mut self,
        request: EditLoftRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before editing a Loft".to_string(),
            ));
        }
        self.validate_active_sketch_references()?;
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit_loft(
                request.feature_id,
                request.loft,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_rib(&mut self, request: RibRequest) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before creating a Rib".to_string(),
            ));
        }
        limo_cad_solid::validate_rib_extent(request.operation, request.extent)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        let feature_id = self.document.alloc_feature_id();
        let next_number = self.rib_count + 1;
        let name = format!("Rib{next_number}");
        let feature = Feature::new(feature_id, name.clone(), FeatureKind::Rib);
        let plan = self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add_rib(feature_id, &name, request, catalog, active)
        })?;
        self.rib_count = next_number;
        Ok(plan)
    }

    pub fn prepare_edit_rib(
        &mut self,
        request: EditRibRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before editing a Rib".to_string(),
            ));
        }
        self.validate_active_sketch_references()?;
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit_rib(
                request.feature_id,
                request.rib,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_solid_fillet(
        &mut self,
        request: SolidFilletRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("creating a solid Fillet")?;
        if request.edge_ids.is_empty() {
            return Err(SessionError::Solid(
                limo_cad_solid::SolidError::EmptyEdgeSelection.to_string(),
            ));
        }
        let feature_id = self.document.alloc_feature_id();
        let next_number = self.fillet_count + 1;
        let name = format!("Fillet{next_number}");
        let feature = Feature::new(feature_id, name.clone(), FeatureKind::Fillet);
        let plan = self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add_fillet(feature_id, &name, request, catalog, active)
        })?;
        self.fillet_count = next_number;
        Ok(plan)
    }

    pub fn prepare_edit_solid_fillet(
        &mut self,
        request: EditSolidFilletRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("editing a solid Fillet")?;
        self.validate_active_sketch_references()?;
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit_fillet(
                request.feature_id,
                request.fillet,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_solid_chamfer(
        &mut self,
        request: SolidChamferRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("creating a solid Chamfer")?;
        if request.edge_ids.is_empty() {
            return Err(SessionError::Solid(
                limo_cad_solid::SolidError::EmptyEdgeSelection.to_string(),
            ));
        }
        let feature_id = self.document.alloc_feature_id();
        let next_number = self.chamfer_count + 1;
        let name = format!("Chamfer{next_number}");
        let feature = Feature::new(feature_id, name.clone(), FeatureKind::Chamfer);
        let plan = self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add_chamfer(feature_id, &name, request, catalog, active)
        })?;
        self.chamfer_count = next_number;
        Ok(plan)
    }

    pub fn prepare_edit_solid_chamfer(
        &mut self,
        request: EditSolidChamferRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("editing a solid Chamfer")?;
        self.validate_active_sketch_references()?;
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit_chamfer(
                request.feature_id,
                request.chamfer,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_hole(&mut self, request: HoleRequest) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("creating a Hole")?;
        let feature_id = self.document.alloc_feature_id();
        let next_number = self.hole_count + 1;
        let name = format!("Hole{next_number}");
        let feature = Feature::new(feature_id, name.clone(), FeatureKind::Hole);
        let plan = self.prepare_new_solid_feature(feature, move |solids, catalog, active| {
            solids.prepare_add_hole(feature_id, &name, request, catalog, active)
        })?;
        self.hole_count = next_number;
        Ok(plan)
    }

    pub fn prepare_edit_hole(
        &mut self,
        request: EditHoleRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("editing a Hole")?;
        self.validate_active_sketch_references()?;
        let active = self.active_feature_ids();
        self.solids
            .prepare_edit_hole(
                request.feature_id,
                request.hole,
                &self.profile_catalog(),
                &active,
            )
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_recompute(&mut self) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before recomputing solids".to_string(),
            ));
        }
        let active = self.active_feature_ids();
        self.solids
            .prepare_recompute_resilient(&self.profile_catalog(), &active)
            .map_err(|error| SessionError::Solid(error.to_string()))
    }

    pub fn prepare_set_rollback(
        &mut self,
        request: SetRollbackRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::Solid(
                "finish the active sketch before moving the rollback marker".to_string(),
            ));
        }
        let index = request
            .rollback_index
            .min(self.document.features().features.len());
        let active = self.active_feature_ids_at(index);
        let plan = self
            .solids
            .prepare_recompute_resilient(&self.profile_catalog_at(index), &active)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        self.document.features_mut().set_rollback_index(index);
        Ok(plan)
    }

    pub fn prepare_delete_feature(
        &mut self,
        request: DeleteFeatureRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("deleting a history feature")?;
        let (index, feature) = self
            .document
            .features()
            .features
            .iter()
            .enumerate()
            .find(|(_, feature)| feature.id == request.feature_id)
            .map(|(index, feature)| (index, feature.clone()))
            .ok_or_else(|| {
                SessionError::Solid(format!("feature {} was not found", request.feature_id.0))
            })?;
        let rollback = self.document.features().rollback_index;
        let next_rollback = if index < rollback {
            rollback.saturating_sub(1)
        } else {
            rollback.min(self.document.features().features.len().saturating_sub(1))
        };
        let active = self
            .document
            .features()
            .features
            .iter()
            .enumerate()
            .filter(|(feature_index, _)| *feature_index != index)
            .take(next_rollback)
            .filter(|(_, feature)| !feature.suppressed)
            .map(|(_, feature)| feature.id)
            .collect::<BTreeSet<_>>();
        let catalog = self
            .profile_catalog()
            .into_iter()
            .filter(|item| item.feature_id != request.feature_id)
            .collect::<Vec<_>>();
        let plan = self
            .solids
            .prepare_delete_feature(request.feature_id, &catalog, &active)
            .map_err(|error| SessionError::Solid(error.to_string()))?;
        self.pending_joint_body_deletion = Some((
            plan.transaction_id,
            self.solids
                .owned_body_ids_for_feature(request.feature_id)
                .into_iter()
                .collect(),
        ));

        self.document.features_mut().remove(request.feature_id);
        match feature.kind {
            FeatureKind::Sketch => {
                self.finished
                    .retain(|finished| finished.feature_id != request.feature_id);
                self.document.remove_sketch_node(&feature.name);
            }
            FeatureKind::ConstructionPlane => {
                let datum_ids = self
                    .datum_planes
                    .iter()
                    .filter(|plane| plane.feature_id == request.feature_id)
                    .map(|plane| plane.datum_id.0)
                    .collect::<Vec<_>>();
                self.datum_planes
                    .retain(|plane| plane.feature_id != request.feature_id);
                for datum_id in datum_ids {
                    self.document.remove_construction_plane_node(datum_id);
                }
            }
            _ => {}
        }
        Ok(plan)
    }

    pub fn prepare_reorder_feature(
        &mut self,
        request: ReorderFeatureRequest,
    ) -> Result<RecomputePlanDto, SessionError> {
        self.ensure_no_active_sketch("reordering history")?;
        if self.document.features().rollback_index != self.document.features().features.len() {
            return Err(SessionError::Solid(
                "move the build cursor to the end before reordering history".to_string(),
            ));
        }
        let original_tree = self.document.features().clone();
        let (dependencies, _) =
            self.timeline_dependencies_and_body_writers(&original_tree.features);
        if !self
            .document
            .features_mut()
            .reorder(request.feature_id, request.target_index)
        {
            return Err(SessionError::Solid(
                "the feature is already in that history position".to_string(),
            ));
        }

        let order = self
            .document
            .features()
            .features
            .iter()
            .map(|feature| feature.id)
            .collect::<Vec<_>>();
        let positions = order
            .iter()
            .enumerate()
            .map(|(index, feature_id)| (*feature_id, index))
            .collect::<BTreeMap<_, _>>();
        if let Some((consumer, producer)) = dependencies.iter().find_map(|(consumer, producers)| {
            producers.iter().find_map(|producer| {
                match (positions.get(producer), positions.get(consumer)) {
                    (Some(producer_index), Some(consumer_index))
                        if producer_index >= consumer_index =>
                    {
                        Some((*consumer, *producer))
                    }
                    _ => None,
                }
            })
        }) {
            let consumer_name = original_tree
                .features
                .iter()
                .find(|feature| feature.id == consumer)
                .map(|feature| feature.name.clone())
                .unwrap_or_else(|| "feature".to_string());
            let producer_name = original_tree
                .features
                .iter()
                .find(|feature| feature.id == producer)
                .map(|feature| feature.name.clone())
                .unwrap_or_else(|| "dependency".to_string());
            *self.document.features_mut() = original_tree;
            return Err(SessionError::Solid(format!(
                "cannot move {consumer_name} before its dependency {producer_name}",
            )));
        }

        let previous_order = original_tree
            .features
            .iter()
            .map(|feature| feature.id)
            .collect::<Vec<_>>();
        if let Err(error) = self.solids.set_feature_order(&order) {
            *self.document.features_mut() = original_tree;
            return Err(SessionError::Solid(error.to_string()));
        }
        let active = self.active_feature_ids();
        match self
            .solids
            .prepare_recompute(&self.profile_catalog(), &active)
        {
            Ok(plan) => Ok(plan),
            Err(error) => {
                *self.document.features_mut() = original_tree;
                let _ = self.solids.set_feature_order(&previous_order);
                Err(SessionError::Solid(format!(
                    "history reorder is not valid: {error}",
                )))
            }
        }
    }

    pub fn cancel_solid_recompute(&mut self, transaction_id: u64) {
        if self
            .pending_project
            .as_ref()
            .is_some_and(|pending| pending.transaction_id == transaction_id)
        {
            self.pending_project = None;
            return;
        }
        self.solids.cancel_pending(transaction_id);
        if self
            .pending_joint_body_deletion
            .as_ref()
            .is_some_and(|(pending_id, _)| *pending_id == transaction_id)
        {
            self.pending_joint_body_deletion = None;
        }
    }

    pub fn commit_solid(
        &mut self,
        request: CommitKernelRequest,
    ) -> Result<SolidUpdateDto, SessionError> {
        self.commit_solid_inner(request, false, None)
    }

    /// Query the support at the sketch's own active history prefix. A pending
    /// project owns its own history; never borrow the previous document's IDs.
    pub fn history_support_queries(&self) -> Vec<limo_cad_solid::HistorySupportQuery> {
        if let Some(pending) = &self.pending_project {
            return pending.manager.history_support_queries();
        }
        let tree = self.document.features();
        let mut previous = None;
        let mut queries = Vec::new();
        for feature in tree
            .features
            .iter()
            .take(tree.rollback_index)
            .filter(|f| !f.suppressed)
        {
            if let Some(finished) = self.finished.iter().find(|f| f.feature_id == feature.id) {
                if let (Some(after_feature), PlaneRef::PlanarFace { face_id }) =
                    (previous, finished.session.plane())
                {
                    queries.push(limo_cad_solid::HistorySupportQuery {
                        sketch_id: feature.id,
                        after_feature,
                        face_id,
                    });
                }
            }
            if feature_changes_solid_topology(feature.kind) {
                previous = Some(feature.id);
            }
        }
        queries
    }

    /// Internal native-host commit: the set must come from the same kernel
    /// recompute as `request.scene`. It is not accepted from serialized clients.
    pub fn commit_solid_with_verified_supports(
        &mut self,
        request: CommitKernelRequest,
        verified: &BTreeSet<FeatureId>,
    ) -> Result<SolidUpdateDto, SessionError> {
        // A partial/failed replay supplies no historical proof. Preserve the
        // ordinary strict scene check instead of marking every face-hosted
        // sketch broken merely because an unrelated later job failed.
        let verified = request.scene.errors.is_empty().then_some(verified);
        self.commit_solid_inner(request, false, verified)
    }

    fn commit_solid_inner(
        &mut self,
        request: CommitKernelRequest,
        restoring_project: bool,
        verified_supports: Option<&BTreeSet<FeatureId>>,
    ) -> Result<SolidUpdateDto, SessionError> {
        if let Some(mut pending) = self.pending_project.take() {
            if pending.transaction_id != request.transaction_id {
                self.pending_project = Some(pending);
                return Err(SessionError::Solid(
                    "stale project recompute result".to_string(),
                ));
            }
            let update = pending
                .manager
                .commit_solid_inner(request, true, verified_supports)?;
            *self = *pending.manager;
            return Ok(update);
        }

        let issued_scene = (!restoring_project
            && self
                .drawings
                .sheets
                .iter()
                .any(|sheet| sheet.release.status == crate::DrawingReleaseStatus::Released))
        .then(|| self.solids.scene_snapshot());
        let scene = self
            .solids
            .commit(request.transaction_id, request.scene)
            .map_err(|error| SessionError::Solid(error.to_string()))?
            .clone();
        if issued_scene
            .as_ref()
            .is_some_and(|prior| prior.bodies != scene.bodies || prior.errors != scene.errors)
        {
            for sheet in &mut self.drawings.sheets {
                if sheet.release.status == crate::DrawingReleaseStatus::Released
                    && !sheet.views.is_empty()
                {
                    sheet.release.status = crate::DrawingReleaseStatus::Draft;
                }
            }
        }
        self.active_named_view = None;
        if let Some((pending_id, deleted_body_ids)) = self.pending_joint_body_deletion.take() {
            if pending_id == request.transaction_id {
                let deleted_body_ids = deleted_body_ids
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>();
                self.assembly
                    .remove_joints_for_deleted_bodies(&deleted_body_ids)
                    .map_err(SessionError::Solid)?;
            }
        }

        let body_ids = scene
            .bodies
            .iter()
            .map(|body| body.id.0)
            .collect::<BTreeSet<_>>();
        self.document
            .retain_body_nodes(|body_id| body_ids.contains(&body_id));
        for body in &scene.bodies {
            if self.document.body_node_id(body.id.0).is_none() {
                self.document.add_body_node(body.id.0, &body.name);
            }
        }
        if scene.errors.is_empty()
            && self.document.features().rollback_index == self.document.features().features.len()
            && self
                .assembly
                .component_structure
                .definitions
                .iter()
                .any(|definition| {
                    definition.promoted
                        && definition
                            .body_ids
                            .iter()
                            .any(|body_id| !body_ids.contains(&body_id.0))
                })
        {
            let (bodies, occurrences) =
                crate::drawing_topology::drawing_component_references(&self.drawings)
                    .map_err(SessionError::Solid)?;
            self.assembly
                .remove_consumed_placeholders(&scene, &bodies, &occurrences)
                .map_err(SessionError::Solid)?;
        }
        self.assembly
            .synchronize_components(&scene)
            .map_err(SessionError::Solid)?;
        self.clear_assembly_solution();
        self.scrub_body_appearances();
        self.scrub_project_visibility();
        self.scrub_named_views();

        for feature in &mut self.document.features_mut().features {
            feature.status = FeatureStatus::Ok;
        }
        let active = self.active_feature_ids();
        let datum_errors = self.refresh_datum_planes(&active);
        self.refresh_projected_face_boundaries(&active);
        for error in &scene.errors {
            self.document.set_feature_status(
                error.feature_id,
                FeatureStatus::Error {
                    message: error.message.clone(),
                },
            );
        }
        for (feature_id, message) in datum_errors {
            self.document
                .set_feature_status(feature_id, FeatureStatus::Error { message });
        }

        let broken = self
            .finished
            .iter()
            .filter_map(|finished| match finished.session.plane() {
                PlaneRef::PlanarFace { face_id }
                    if active.contains(&finished.feature_id)
                        && verified_supports.map_or_else(
                            || !self.solids.has_face(face_id),
                            |verified| !verified.contains(&finished.feature_id),
                        ) =>
                {
                    Some((
                        finished.feature_id,
                        finished.session.name().to_string(),
                        format!("face {}", face_id.0),
                    ))
                }
                PlaneRef::DatumPlane { datum_id }
                    if active.contains(&finished.feature_id)
                        && self.resolve_datum_basis(datum_id, &active).is_none() =>
                {
                    Some((
                        finished.feature_id,
                        finished.session.name().to_string(),
                        format!("construction plane {}", datum_id.0),
                    ))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (feature_id, sketch_name, reference) in broken {
            let message =
                format!("Broken reference: {reference} used by {sketch_name} no longer exists");
            self.document.set_feature_status(
                feature_id,
                FeatureStatus::Error {
                    message: message.clone(),
                },
            );
            let downstream = self
                .solids
                .definitions()
                .iter()
                .filter(|definition| definition.sketch_name == sketch_name)
                .map(|definition| definition.feature_id)
                .chain(
                    self.solids
                        .revolve_definitions()
                        .iter()
                        .filter(|definition| definition.sketch_name == sketch_name)
                        .map(|definition| definition.feature_id),
                )
                .chain(
                    self.solids
                        .sweep_definitions()
                        .iter()
                        .filter(|definition| {
                            definition.profile.sketch_name == sketch_name
                                || definition.path_sketch_name == sketch_name
                        })
                        .map(|definition| definition.feature_id),
                )
                .chain(
                    self.solids
                        .loft_definitions()
                        .iter()
                        .filter(|definition| {
                            definition
                                .sections
                                .iter()
                                .any(|section| section.sketch_name == sketch_name)
                        })
                        .map(|definition| definition.feature_id),
                )
                .chain(
                    self.solids
                        .rib_definitions()
                        .iter()
                        .filter(|definition| definition.sketch_name == sketch_name)
                        .map(|definition| definition.feature_id),
                )
                .collect::<Vec<_>>();
            for feature in downstream {
                self.document.set_feature_status(
                    feature,
                    FeatureStatus::Error {
                        message: format!(
                            "Broken reference: source sketch {} has no support face",
                            sketch_name
                        ),
                    },
                );
            }
        }

        Ok(SolidUpdateDto {
            document: self.document_dto(),
            scene,
        })
    }

    fn active_feature_ids(&self) -> BTreeSet<FeatureId> {
        self.active_feature_ids_at(self.document.features().rollback_index)
    }

    /// Capture the dependency edges represented by the current valid
    /// timeline. Reordering may move independent branches, or move a
    /// producer earlier / consumer later, but it may not invert one of these
    /// edges. Stable ids remain unchanged.
    fn timeline_dependencies_and_body_writers(
        &self,
        features: &[Feature],
    ) -> (
        BTreeMap<FeatureId, BTreeSet<FeatureId>>,
        BTreeMap<BodyId, FeatureId>,
    ) {
        #[derive(Default)]
        struct BodyAccess {
            inputs: BTreeSet<BodyId>,
            writes: BTreeSet<BodyId>,
            outputs: BTreeSet<BodyId>,
        }

        let mut dependencies = BTreeMap::<FeatureId, BTreeSet<FeatureId>>::new();
        let mut sketch_inputs = BTreeMap::<FeatureId, BTreeSet<String>>::new();
        let mut plane_inputs = BTreeMap::<FeatureId, Vec<PlaneRef>>::new();
        let mut body_access = BTreeMap::<FeatureId, BodyAccess>::new();

        let mut record_solid = |feature_id: FeatureId,
                                operation: ExtrudeOperation,
                                targets: &[BodyId],
                                outputs: &[BodyId],
                                additional_inputs: &[BodyId]| {
            let access = body_access.entry(feature_id).or_default();
            access.inputs.extend(additional_inputs.iter().copied());
            match operation {
                ExtrudeOperation::NewBody => {
                    access.outputs.extend(outputs.iter().copied());
                }
                ExtrudeOperation::Join | ExtrudeOperation::Cut | ExtrudeOperation::Intersect => {
                    access.inputs.extend(targets.iter().copied());
                    access.writes.extend(targets.iter().copied());
                }
            }
        };

        for definition in self.solids.definitions() {
            if definition.source_face.is_none() {
                sketch_inputs
                    .entry(definition.feature_id)
                    .or_default()
                    .insert(definition.sketch_name.clone());
            }
            let source_body = definition.source_face.map(|source| source.body_id);
            record_solid(
                definition.feature_id,
                definition.operation,
                &definition.target_body_ids,
                &definition.new_body_ids,
                source_body.as_slice(),
            );
            if let ExtrudeExtent::ToFace { face_id } = definition.extent {
                plane_inputs
                    .entry(definition.feature_id)
                    .or_default()
                    .push(PlaneRef::PlanarFace { face_id });
            }
        }
        for definition in self.solids.revolve_definitions() {
            sketch_inputs
                .entry(definition.feature_id)
                .or_default()
                .insert(definition.sketch_name.clone());
            record_solid(
                definition.feature_id,
                definition.operation,
                &definition.target_body_ids,
                &definition.new_body_ids,
                &[],
            );
        }
        for definition in self.solids.sweep_definitions() {
            let inputs = sketch_inputs.entry(definition.feature_id).or_default();
            inputs.insert(definition.profile.sketch_name.clone());
            inputs.insert(definition.path_sketch_name.clone());
            if let Some(guide) = &definition.guide_rail {
                inputs.insert(guide.sketch_name.clone());
            }
            record_solid(
                definition.feature_id,
                definition.operation,
                &definition.target_body_ids,
                &[definition.new_body_id],
                &[],
            );
        }
        for definition in self.solids.loft_definitions() {
            let inputs = sketch_inputs.entry(definition.feature_id).or_default();
            inputs.extend(
                definition
                    .sections
                    .iter()
                    .map(|section| section.sketch_name.clone()),
            );
            if let Some(centerline) = &definition.centerline {
                inputs.insert(centerline.sketch_name.clone());
            }
            if let Some(guide) = &definition.guide_rail {
                inputs.insert(guide.sketch_name.clone());
            }
            record_solid(
                definition.feature_id,
                definition.operation,
                &definition.target_body_ids,
                &[definition.new_body_id],
                &[],
            );
        }
        for definition in self.solids.rib_definitions() {
            sketch_inputs
                .entry(definition.feature_id)
                .or_default()
                .insert(definition.sketch_name.clone());
            record_solid(
                definition.feature_id,
                definition.operation,
                &definition.target_body_ids,
                &definition.new_body_ids,
                &[],
            );
            if let Some(RibExtent::ToFace { face_id }) = definition.extent {
                plane_inputs
                    .entry(definition.feature_id)
                    .or_default()
                    .push(PlaneRef::PlanarFace { face_id });
            }
        }
        for definition in self.solids.fillet_definitions() {
            let access = body_access.entry(definition.feature_id).or_default();
            access.inputs.insert(definition.body_id);
            access.writes.insert(definition.body_id);
        }
        for definition in self.solids.chamfer_definitions() {
            let access = body_access.entry(definition.feature_id).or_default();
            access.inputs.insert(definition.body_id);
            access.writes.insert(definition.body_id);
        }
        for definition in self.solids.hole_definitions() {
            let access = body_access.entry(definition.feature_id).or_default();
            access.inputs.insert(definition.body_id);
            access.writes.insert(definition.body_id);
            let refs = if definition.positions.is_empty() {
                definition.position_reference.iter().collect::<Vec<_>>()
            } else {
                definition
                    .positions
                    .iter()
                    .filter_map(|position| position.position_reference.as_ref())
                    .collect::<Vec<_>>()
            };
            sketch_inputs
                .entry(definition.feature_id)
                .or_default()
                .extend(
                    refs.into_iter()
                        .map(|reference| reference.sketch_name.clone()),
                );
        }
        for definition in self.solids.body_feature_definitions() {
            match definition {
                BodyFeatureDefinitionDto::ExternalThread {
                    feature_id,
                    body_id,
                    ..
                } => {
                    let access = body_access.entry(*feature_id).or_default();
                    access.inputs.insert(*body_id);
                    access.writes.insert(*body_id);
                }
                BodyFeatureDefinitionDto::MoveCopy {
                    feature_id,
                    body_ids,
                    copy,
                    result_body_ids,
                    ..
                } => {
                    let access = body_access.entry(*feature_id).or_default();
                    access.inputs.extend(body_ids.iter().copied());
                    if *copy {
                        access.outputs.extend(result_body_ids.iter().copied());
                    } else {
                        access.writes.extend(body_ids.iter().copied());
                    }
                }
                BodyFeatureDefinitionDto::Shell {
                    feature_id,
                    body_id,
                    ..
                } => {
                    let access = body_access.entry(*feature_id).or_default();
                    access.inputs.insert(*body_id);
                    access.writes.insert(*body_id);
                }
                BodyFeatureDefinitionDto::Mirror {
                    feature_id,
                    body_ids,
                    plane,
                    new_body_ids,
                    ..
                } => {
                    let access = body_access.entry(*feature_id).or_default();
                    access.inputs.extend(body_ids.iter().copied());
                    access.outputs.extend(new_body_ids.iter().copied());
                    plane_inputs.entry(*feature_id).or_default().push(*plane);
                }
                BodyFeatureDefinitionDto::RectangularPattern {
                    feature_id,
                    body_ids,
                    new_body_ids,
                    ..
                }
                | BodyFeatureDefinitionDto::CircularPattern {
                    feature_id,
                    body_ids,
                    new_body_ids,
                    ..
                } => {
                    let access = body_access.entry(*feature_id).or_default();
                    access.inputs.extend(body_ids.iter().copied());
                    access.outputs.extend(new_body_ids.iter().copied());
                }
                BodyFeatureDefinitionDto::Combine {
                    feature_id,
                    target_body_id,
                    tool_body_ids,
                    ..
                } => {
                    let access = body_access.entry(*feature_id).or_default();
                    access.inputs.insert(*target_body_id);
                    access.inputs.extend(tool_body_ids.iter().copied());
                    access.writes.insert(*target_body_id);
                    access.writes.extend(tool_body_ids.iter().copied());
                }
                BodyFeatureDefinitionDto::SplitBody {
                    feature_id,
                    body_id,
                    plane,
                    new_body_id,
                    ..
                } => {
                    let access = body_access.entry(*feature_id).or_default();
                    access.inputs.insert(*body_id);
                    access.writes.insert(*body_id);
                    access.outputs.insert(*new_body_id);
                    plane_inputs.entry(*feature_id).or_default().push(*plane);
                }
                BodyFeatureDefinitionDto::ImportStep {
                    feature_id,
                    body_id,
                    ..
                } => {
                    body_access
                        .entry(*feature_id)
                        .or_default()
                        .outputs
                        .insert(*body_id);
                }
            }
        }

        for finished in &self.finished {
            plane_inputs
                .entry(finished.feature_id)
                .or_default()
                .push(finished.session.plane());
        }
        for definition in &self.datum_planes {
            let planes = plane_inputs.entry(definition.feature_id).or_default();
            match definition.source {
                DatumPlaneSourceDto::Offset { reference, .. } => planes.push(reference),
                DatumPlaneSourceDto::Midplane { first, second } => {
                    planes.extend([first, second]);
                }
                DatumPlaneSourceDto::AtAngle {
                    reference, body_id, ..
                } => {
                    planes.push(reference);
                    body_access
                        .entry(definition.feature_id)
                        .or_default()
                        .inputs
                        .insert(body_id);
                }
            }
        }

        let sketch_features = self
            .finished
            .iter()
            .map(|finished| (finished.session.name().to_string(), finished.feature_id))
            .collect::<BTreeMap<_, _>>();
        let datum_features = self
            .datum_planes
            .iter()
            .map(|plane| (plane.datum_id, plane.feature_id))
            .collect::<BTreeMap<_, _>>();
        let face_bodies = self
            .solids
            .scene()
            .bodies
            .iter()
            .flat_map(|body| body.faces.iter().map(|face| (face.id, body.id)))
            .collect::<BTreeMap<_, _>>();
        let mut last_writer = BTreeMap::<BodyId, FeatureId>::new();

        for feature in features {
            if let Some(inputs) = sketch_inputs.get(&feature.id) {
                for sketch_name in inputs {
                    if let Some(producer) = sketch_features.get(sketch_name) {
                        if *producer != feature.id {
                            dependencies
                                .entry(feature.id)
                                .or_default()
                                .insert(*producer);
                        }
                    }
                }
            }
            if let Some(inputs) = plane_inputs.get(&feature.id) {
                for reference in inputs {
                    match *reference {
                        PlaneRef::OriginPlane { .. } => {}
                        PlaneRef::DatumPlane { datum_id } => {
                            if let Some(producer) = datum_features.get(&datum_id) {
                                if *producer != feature.id {
                                    dependencies
                                        .entry(feature.id)
                                        .or_default()
                                        .insert(*producer);
                                }
                            }
                        }
                        PlaneRef::PlanarFace { face_id } => {
                            if let Some(producer) = face_bodies
                                .get(&face_id)
                                .and_then(|body_id| last_writer.get(body_id))
                            {
                                if *producer != feature.id {
                                    dependencies
                                        .entry(feature.id)
                                        .or_default()
                                        .insert(*producer);
                                }
                            }
                        }
                    }
                }
            }
            if let Some(access) = body_access.get(&feature.id) {
                for body_id in access.inputs.iter().chain(access.writes.iter()) {
                    if let Some(producer) = last_writer.get(body_id) {
                        if *producer != feature.id {
                            dependencies
                                .entry(feature.id)
                                .or_default()
                                .insert(*producer);
                        }
                    }
                }
                for body_id in access.outputs.iter().chain(access.writes.iter()) {
                    last_writer.insert(*body_id, feature.id);
                }
            }
        }
        (dependencies, last_writer)
    }

    fn refresh_datum_planes(&mut self, active: &BTreeSet<FeatureId>) -> Vec<(FeatureId, String)> {
        let mut errors = Vec::new();
        let mut working = self.datum_planes.clone();
        let feature_order = self
            .document
            .features()
            .features
            .iter()
            .enumerate()
            .map(|(index, feature)| (feature.id, index))
            .collect::<BTreeMap<_, _>>();
        let mut order = (0..working.len()).collect::<Vec<_>>();
        order.sort_by_key(|index| {
            feature_order
                .get(&working[*index].feature_id)
                .copied()
                .unwrap_or(usize::MAX)
        });
        for index in order {
            if !active.contains(&working[index].feature_id) {
                continue;
            }

            if datum_source_reads_solid_topology(&working[index].source)
                && !self.scene_matches_history_stage(working[index].feature_id)
            {
                continue;
            }
            let mut source = working[index].source.clone();
            match resolve_datum_source(&self.solids, &working, active, &mut source) {
                Ok(basis) => {
                    working[index].source = source;
                    working[index].basis = basis;
                }
                Err(error) => errors.push((
                    working[index].feature_id,
                    format!("Broken construction-plane reference: {error}"),
                )),
            }
        }
        self.datum_planes = working;
        for plane in &self.datum_planes {
            if active.contains(&plane.feature_id) {
                self.solids
                    .refresh_datum_plane_basis(plane.datum_id, plane.basis);
            }
        }
        self.refresh_datum_sketch_bases();
        errors
    }

    fn refresh_datum_sketch_bases(&mut self) {
        let bases = self
            .datum_planes
            .iter()
            .map(|plane| (plane.datum_id, plane.basis))
            .collect::<std::collections::HashMap<_, _>>();
        for finished in &mut self.finished {
            if let PlaneRef::DatumPlane { datum_id } = finished.session.plane() {
                if let Some(basis) = bases.get(&datum_id) {
                    finished.session.set_basis(*basis);
                }
            }
        }
        if let Some(session) = &mut self.active {
            if let PlaneRef::DatumPlane { datum_id } = session.plane() {
                if let Some(basis) = bases.get(&datum_id) {
                    session.set_basis(*basis);
                }
            }
        }
    }

    /// Install the external references a face-hosted sketch needs:
    /// support-edge snap midpoints and the projected support-face boundary.
    ///
    /// Both are refreshed from stable ids, so a sketch that leaves the face must
    /// clear them rather than keep stale support geometry. The projected
    /// boundary is what lets geometry drawn against a face edge close a region
    /// (see `profile_catalog_item`).
    fn install_support_references(
        &self,
        session: &mut SketchSession,
        plane: PlaneRef,
        basis: PlaneBasis,
    ) {
        if let PlaneRef::PlanarFace { face_id } = plane {
            session.set_reference_midpoints(support_edge_midpoints(&self.solids, face_id, basis));
            session.set_projected_edges(projected_face_boundary_edges(
                &self.solids,
                face_id,
                basis,
            ));
        } else {
            session.set_reference_midpoints(Vec::new());
            session.set_projected_edges(Vec::new());
        }
    }

    /// Rebuild the projected support-face boundary of every active face-hosted
    /// sketch after a kernel commit. A recompute is exactly when the stable
    /// edge ids resolve to new tessellation. Saved projections bootstrap replay
    /// before the kernel scene exists.
    ///
    /// A sketch whose stage is masked by a later topology writer keeps its
    /// previous projection, matching how datum sketches keep their basis.
    fn refresh_projected_face_boundaries(&mut self, active: &BTreeSet<FeatureId>) {
        let mut refreshed = Vec::new();
        for (index, finished) in self.finished.iter().enumerate() {
            if !active.contains(&finished.feature_id)
                || !self.scene_matches_history_stage(finished.feature_id)
                || !finished.session.projects_support_boundary()
            {
                continue;
            }
            let PlaneRef::PlanarFace { face_id } = finished.session.plane() else {
                continue;
            };
            refreshed.push((
                index,
                projected_face_boundary_edges(&self.solids, face_id, finished.session.basis()),
            ));
        }
        for (index, projected) in refreshed {
            self.finished[index].session.set_projected_edges(projected);
        }
        let active_projection = match &self.active {
            Some(session)
                if session.projects_support_boundary()
                    && self
                        .active_feature_id
                        .is_some_and(|id| self.scene_matches_history_stage(id)) =>
            {
                match session.plane() {
                    PlaneRef::PlanarFace { face_id } => Some(projected_face_boundary_edges(
                        &self.solids,
                        face_id,
                        session.basis(),
                    )),
                    _ => None,
                }
            }
            _ => None,
        };
        if let (Some(session), Some(projected)) = (&mut self.active, active_projection) {
            session.set_projected_edges(projected);
        }
    }

    fn ensure_no_active_sketch(&self, action: &str) -> Result<(), SessionError> {
        if self.active.is_some() {
            Err(SessionError::Solid(format!(
                "finish the active sketch before {action}"
            )))
        } else {
            Ok(())
        }
    }

    /// Whether the current OCCT scene represents the topology visible at one
    /// feature's position in history. Sketch and datum entries do not alter a
    /// body; every other active feature can. Earlier face/edge references may
    /// only be dereferenced when no such writer follows them in the active
    /// prefix. This is the temporal half of persistent topology naming.
    fn scene_matches_history_stage(&self, feature_id: FeatureId) -> bool {
        let tree = self.document.features();
        let Some(position) = tree
            .features
            .iter()
            .position(|feature| feature.id == feature_id)
        else {
            return false;
        };
        if position >= tree.rollback_index {
            return false;
        }
        !tree
            .features
            .iter()
            .take(tree.rollback_index)
            .skip(position + 1)
            .any(|feature| !feature.suppressed && feature_changes_solid_topology(feature.kind))
    }

    fn active_feature_ids_at(&self, rollback_index: usize) -> BTreeSet<FeatureId> {
        self.document
            .features()
            .features
            .iter()
            .take(rollback_index)
            .filter(|feature| !feature.suppressed)
            .map(|feature| feature.id)
            .collect()
    }

    fn validate_active_sketch_references(&self) -> Result<(), SessionError> {
        let active = self.active_feature_ids();
        for finished in &self.finished {
            if !active.contains(&finished.feature_id) {
                continue;
            }
            if let PlaneRef::PlanarFace { face_id } = finished.session.plane() {
                if !self.solids.has_face(face_id) {
                    return Err(SessionError::BrokenReference(format!(
                        "face {} used by {} no longer exists",
                        face_id.0,
                        finished.session.name()
                    )));
                }
            }
            if let PlaneRef::DatumPlane { datum_id } = finished.session.plane() {
                if self.resolve_datum_basis(datum_id, &active).is_none() {
                    return Err(SessionError::BrokenReference(format!(
                        "construction plane {} used by {} no longer exists",
                        datum_id.0,
                        finished.session.name()
                    )));
                }
            }
        }
        Ok(())
    }

    /// Sketch Palette "Snap" toggle: applies to the active session and is
    /// remembered for future ones.
    pub fn set_grid_snap(
        &mut self,
        request: SetGridSnapRequest,
    ) -> Result<SketchDto, SessionError> {
        self.grid_snap = request.enabled;
        let session = self.active.as_mut().ok_or(SessionError::NoActiveSketch)?;
        session.set_grid_snap(request.enabled);
        Ok(session.dto())
    }

    /// Set the current adaptive sketch-grid spacing. Unlike the Snap toggle,
    /// this is valid without an active sketch so the next session inherits
    /// the viewport's current zoom level.
    pub fn set_grid_step(&mut self, request: SetGridStepRequest) -> Result<(), SessionError> {
        if !request.step_mm.is_finite()
            || !(MIN_GRID_STEP_MM..=MAX_GRID_STEP_MM).contains(&request.step_mm)
        {
            return Err(SessionError::InvalidGridStep(request.step_mm));
        }
        if let Some(session) = self.active.as_mut() {
            session.set_grid_step(request.step_mm)?;
        }
        self.grid_step = request.step_mm;
        Ok(())
    }

    fn active_mut(&mut self) -> Result<&mut SketchSession, SessionError> {
        self.active.as_mut().ok_or(SessionError::NoActiveSketch)
    }

    /// The native host already serializes engine calls. Restore view settings
    /// after every dispatch so one window's zoom cannot leak into another caller.
    pub(crate) fn with_viewport_snap(
        &mut self,
        context: crate::dto::ViewportSnapContext,
        dispatch: impl FnOnce(&mut Self) -> String,
    ) -> Result<String, SessionError> {
        let previous = self.active_mut()?.replace_viewport_snap(context)?;
        let result = dispatch(self);
        self.active_mut()?.restore_viewport_snap(previous);
        Ok(result)
    }

    pub fn preview_creation_point(
        &self,
        request: crate::dto::CreationPointPreviewRequest,
    ) -> Result<PreviewDto, SessionError> {
        Ok(self
            .active
            .as_ref()
            .ok_or(SessionError::NoActiveSketch)?
            .preview_creation_point(request))
    }

    pub fn preview_segment(&self, request: SegmentRequest) -> Result<PreviewDto, SessionError> {
        let session = self.active.as_ref().ok_or(SessionError::NoActiveSketch)?;
        Ok(session.preview_segment(request.from, request.to_raw, request.ctrl_held))
    }

    pub fn preview_creation(
        &self,
        request: crate::dto::CreationPreviewRequest,
    ) -> Result<crate::dto::CreationPreviewDto, SessionError> {
        self.active
            .as_ref()
            .ok_or(SessionError::NoActiveSketch)?
            .preview_creation(&request)
    }

    /// Evaluate an expression against the active sketch's parameters (D9
    /// formula previews in dynamic input).
    pub fn eval_expression(
        &self,
        request: EvalExpressionRequest,
    ) -> Result<EvalExpressionResult, SessionError> {
        let session = self.active.as_ref().ok_or(SessionError::NoActiveSketch)?;
        let value = session.eval_text(&request.text)?;
        Ok(EvalExpressionResult { value })
    }

    pub fn add_line(&mut self, request: SegmentRequest) -> Result<AddLineResult, SessionError> {
        self.active_mut()?
            .add_line(request.from, request.to_raw, request.ctrl_held)
    }

    pub fn preview_segment_locked(
        &self,
        request: LockedSegmentRequest,
    ) -> Result<PreviewDto, SessionError> {
        let session = self.active.as_ref().ok_or(SessionError::NoActiveSketch)?;

        let length_mm =
            session.positive_input(request.length_text.as_deref(), request.length_mm)?;
        let angle_deg = match &request.angle_text {
            Some(t) => Some(session.eval_text(t)?),
            None => request.angle_deg,
        };
        Ok(session.preview_segment_locked(
            request.from,
            (length_mm, angle_deg),
            request.to_hint,
            request.ctrl_held,
            (
                request.tracking,
                request.intersection,
                request.from_crossing,
                request.to_crossing,
            ),
        ))
    }

    pub fn add_line_locked(
        &mut self,
        request: LockedSegmentRequest,
    ) -> Result<AddLineResult, SessionError> {
        self.active_mut()?.add_line_locked(&request)
    }

    pub fn add_point(&mut self, request: PointRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_point_on_selective(
            request.position,
            request.coincident_with,
            request.ctrl_held,
        )
    }

    pub fn add_line_midpoint(
        &mut self,
        request: MidpointLineRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?
            .add_line_midpoint(request.mid_raw, request.end_raw, request.ctrl_held)
    }

    pub fn add_rectangle(&mut self, request: RectangleRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_rectangle_selective(
            request.mode,
            request.p1,
            request.p2,
            request.ctrl_held,
        )
    }

    pub fn add_rectangle_locked(
        &mut self,
        request: LockedRectangleRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_rectangle_locked(&request)
    }

    /// An unfinished rectangle has no preview until both axes have a usable
    /// extent. Invalid typed sizes still fail; committing remains strict.
    pub fn preview_rectangle_locked(
        &self,
        request: LockedRectangleRequest,
    ) -> Result<Option<[crate::Vec2; 2]>, SessionError> {
        let sketch = self.active.as_ref().ok_or(SessionError::NoActiveSketch)?;
        match sketch.preview_rectangle_locked(&request) {
            Ok(points) => Ok(Some(points)),
            Err(SessionError::DegenerateSegment) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn preview_circle_locked(
        &self,
        request: LockedCircleRequest,
    ) -> Result<[crate::Vec2; 2], SessionError> {
        self.active
            .as_ref()
            .ok_or(SessionError::NoActiveSketch)?
            .preview_circle_locked(&request)
    }

    pub fn add_circle(&mut self, request: CircleRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_circle_selective(
            request.mode,
            request.p1,
            request.p2,
            request.ctrl_held,
        )
    }

    pub fn add_circle_locked(
        &mut self,
        request: LockedCircleRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_circle_locked(&request)
    }

    pub fn add_slot(&mut self, request: SlotRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_slot(&request)
    }

    pub fn add_spline(&mut self, request: SplineRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_spline(&request)
    }

    pub fn add_arc_3pt(&mut self, request: Arc3PointRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_arc_3pt_selective(
            request.p1,
            request.p2,
            request.p3,
            request.ctrl_held,
        )
    }

    pub fn add_arc_center(
        &mut self,
        request: ArcCenterRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_arc_center_locked(
            (request.center, request.start, request.sweep),
            request.ctrl_held,
            request.radius_mm,
            request.radius_text.as_deref(),
            request.angle_text.as_deref(),
            request.sweep_rad,
        )
    }

    pub fn add_constraint(
        &mut self,
        constraint: Constraint,
    ) -> Result<AddConstraintResult, SessionError> {
        self.active_mut()?.add_constraint(constraint)
    }

    pub fn add_constraints(
        &mut self,
        request: ConstraintBatchRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_constraints(request.constraints)
    }

    pub fn add_dimension(&mut self, request: DimensionRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.add_dimension(request)
    }

    pub fn edit_dimension(
        &mut self,
        request: EditDimensionRequest,
    ) -> Result<AddConstraintResult, SessionError> {
        self.active_mut()?.edit_dimension(request)
    }

    pub fn set_dimension_mode(
        &mut self,
        request: SetDimensionModeRequest,
    ) -> Result<AddConstraintResult, SessionError> {
        self.active_mut()?.set_dimension_mode(request)
    }

    pub fn move_dimension(
        &mut self,
        request: MoveDimensionRequest,
    ) -> Result<AddConstraintResult, SessionError> {
        self.active_mut()?.move_dimension(request)
    }

    pub fn delete_dimension(
        &mut self,
        constraint_id: ConstraintId,
    ) -> Result<AddConstraintResult, SessionError> {
        self.active_mut()?.delete_dimension(constraint_id)
    }

    pub fn delete_constraint(
        &mut self,
        constraint_id: ConstraintId,
    ) -> Result<AddConstraintResult, SessionError> {
        self.active_mut()?.delete_constraint(constraint_id)
    }

    /// ISO/aligned dimension style toggle (document setting, D4.5).
    pub fn set_dimension_style(
        &mut self,
        request: SetDimensionStyleRequest,
    ) -> Result<SketchDto, SessionError> {
        self.document.settings_mut().dimension_style = request.style;
        let session = self.active.as_mut().ok_or(SessionError::NoActiveSketch)?;
        session.set_dimension_style(request.style);
        Ok(session.dto())
    }

    pub fn fillet_preview(
        &self,
        request: &FilletRequest,
    ) -> Result<FilletPreviewDto, SessionError> {
        self.active
            .as_ref()
            .ok_or(SessionError::NoActiveSketch)?
            .fillet_preview(request)
    }

    pub fn fillet_lines(&mut self, request: FilletRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.fillet_lines(&request)
    }

    pub fn chamfer_lines(&mut self, request: ChamferRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.chamfer_lines(&request)
    }

    pub fn chamfer_preview(
        &self,
        request: ChamferRequest,
    ) -> Result<crate::PreviewCurve, SessionError> {
        self.active
            .as_ref()
            .ok_or(SessionError::NoActiveSketch)?
            .chamfer_preview(&request)
    }

    pub fn offset_preview(
        &self,
        request: &OffsetRequest,
    ) -> Result<OffsetPreviewDto, SessionError> {
        self.active
            .as_ref()
            .ok_or(SessionError::NoActiveSketch)?
            .offset_preview(request)
    }

    pub fn offset_curve(&mut self, request: OffsetRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.offset_curve_op(&request)
    }

    pub fn trim_preview(&self, request: &TrimRequest) -> Result<TrimPreviewDto, SessionError> {
        self.active
            .as_ref()
            .ok_or(SessionError::NoActiveSketch)?
            .trim_preview(request)
    }

    pub fn trim_entity(&mut self, request: TrimRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.trim_entity(&request)
    }

    pub fn extend_entity(&mut self, request: ExtendRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.extend_entity(&request)
    }

    pub fn break_curve(&mut self, request: BreakRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.break_curve(&request)
    }

    pub fn mirror_entities(&mut self, request: MirrorRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.mirror_entities(&request)
    }

    pub fn rectangular_pattern(
        &mut self,
        request: RectangularPatternRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?.rectangular_pattern(&request)
    }

    pub fn circular_pattern(
        &mut self,
        request: CircularPatternRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?.circular_pattern(&request)
    }

    pub fn move_copy_entities(
        &mut self,
        request: MoveCopyRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?.move_copy_entities(&request)
    }

    pub fn scale_entities(&mut self, request: ScaleRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.scale_entities(&request)
    }

    pub fn polygon_create(&mut self, request: PolygonRequest) -> Result<ToolResult, SessionError> {
        self.active_mut()?.polygon_create(&request)
    }

    pub fn toggle_fix(&mut self, entity: EntityId) -> Result<AddConstraintResult, SessionError> {
        self.active_mut()?.toggle_fix(entity)
    }

    pub fn toggle_fix_entities(
        &mut self,
        request: ToggleFixBatchRequest,
    ) -> Result<ToolResult, SessionError> {
        self.active_mut()?.toggle_fix_entities(request.entity_ids)
    }

    pub fn move_point(
        &mut self,
        request: MovePointRequest,
    ) -> Result<MovePointResult, SessionError> {
        self.active_mut()?.move_point(request)
    }

    pub fn delete_entity(&mut self, id: EntityId) -> Result<DeleteEntityResult, SessionError> {
        self.active_mut()?.delete_entity(id)
    }

    pub fn delete_entities(
        &mut self,
        ids: &[EntityId],
    ) -> Result<DeleteEntityResult, SessionError> {
        self.active_mut()?.delete_entities(ids)
    }

    pub fn undo(&mut self) -> Result<UndoResult, SessionError> {
        self.active_mut()?.undo()
    }

    pub fn redo(&mut self) -> Result<UndoResult, SessionError> {
        self.active_mut()?.redo()
    }
}

/// Find midpoint snap candidates on the support face. Edge-to-face
/// adjacency is not part of the render DTO yet, so membership is resolved
/// geometrically: every tessellated point on the edge must lie on the
/// selected planar face. The midpoint follows polyline arc length rather
/// than simply averaging endpoints, which also behaves correctly for
/// tessellated arcs.
fn support_edge_midpoints(
    solids: &SolidDocument,
    face_id: FaceId,
    basis: PlaneBasis,
) -> Vec<(EdgeId, crate::geometry::Vec2)> {
    let Some(body) = solids
        .scene()
        .bodies
        .iter()
        .find(|body| body.faces.iter().any(|face| face.id == face_id))
    else {
        return Vec::new();
    };

    body.edges
        .iter()
        .filter_map(|edge| {
            if edge.points.len() < 2
                || edge.points.iter().any(|point| {
                    dot3(sub3(point3_array(*point), basis.origin), basis.normal).abs() > 1e-4
                })
            {
                return None;
            }
            let lengths = edge
                .points
                .windows(2)
                .map(|pair| length3(sub3(point3_array(pair[1]), point3_array(pair[0]))))
                .collect::<Vec<_>>();
            let total = lengths.iter().sum::<f64>();
            if total <= 1e-9 {
                return None;
            }
            let target = total * 0.5;
            let mut traversed = 0.0;
            for (index, segment_length) in lengths.iter().copied().enumerate() {
                if traversed + segment_length + 1e-12 >= target {
                    let a = point3_array(edge.points[index]);
                    let b = point3_array(edge.points[index + 1]);
                    let t = ((target - traversed) / segment_length).clamp(0.0, 1.0);
                    let point = add3(a, scale3(sub3(b, a), t));
                    let local = basis.to_2d(point);
                    return Some((edge.id, crate::geometry::Vec2::new(local[0], local[1])));
                }
                traversed += segment_length;
            }
            None
        })
        .collect()
}

/// Project the boundary edges of a support face into sketch coordinates.
///
/// Only the face's own edges (`FaceDto::edge_keys`) are projected: a coplanar
/// edge belonging to a neighbouring face is not part of the region the user
/// selected as the sketch plane. Scenes that publish no boundary keys (some
/// imported or assembly bodies) fall back to every coplanar edge, which is the
/// same set the snap references use.
///
/// Saved with the sketch and refreshed only from its own history-stage scene.
fn projected_face_boundary_edges(
    solids: &SolidDocument,
    face_id: FaceId,
    basis: PlaneBasis,
) -> Vec<ProjectedEdgeDto> {
    let Some(body) = solids
        .scene()
        .bodies
        .iter()
        .find(|body| body.faces.iter().any(|face| face.id == face_id))
    else {
        return Vec::new();
    };
    let boundary_keys = body
        .faces
        .iter()
        .find(|face| face.id == face_id)
        .map(|face| face.edge_keys.clone())
        .unwrap_or_default();
    let mut candidates = body
        .edges
        .iter()
        .filter(|edge| {
            edge.points.len() >= 2
                && (boundary_keys.is_empty() || boundary_keys.contains(&edge.key))
                && edge.points.iter().all(|point| {
                    dot3(sub3(point3_array(*point), basis.origin), basis.normal).abs() <= 1e-4
                })
        })
        .collect::<Vec<_>>();

    candidates.sort_by_key(|edge| edge.id.0);
    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, edge)| {
            let points = edge
                .points
                .iter()
                .map(|point| {
                    let local = basis.to_2d(point3_array(*point));
                    crate::geometry::Vec2::new(local[0], local[1])
                })
                .collect::<Vec<_>>();
            let span = points
                .windows(2)
                .map(|pair| pair[0].distance(pair[1]))
                .sum::<f64>();
            if span <= 1e-9 {
                return None;
            }
            let circle = edge.circle.as_ref().and_then(|circle| {
                let center = basis.to_2d(point3_array(circle.center));
                let center = crate::geometry::Vec2::new(center[0], center[1]);
                let radius_tolerance = (circle.radius * 1e-4).max(1e-4);
                let sampled = points.first().map(|point| point.distance(center))?;
                ((sampled - circle.radius).abs() <= radius_tolerance).then_some(
                    crate::dto::ProjectedCircleDto {
                        center,
                        radius: circle.radius,
                        closed: circle.closed,
                    },
                )
            });
            Some(ProjectedEdgeDto {
                id: PROJECTED_EDGE_ID_BASE + index as u64,
                edge_id: edge.id,
                points,
                circle,
            })
        })
        .collect()
}

/// Recover the kernel boundary curve of one projected-edge group.
///
/// The projection carries the exact circle when the body edge is circular, so
/// the kernel still receives one analytic arc instead of its tessellation
/// chords. Anything else stays a straight line (two samples) or an explicit
/// polyline.
fn projected_profile_curve(
    projected: &ProjectedEdgeDto,
    entity_id: u64,
    path: &[Point2Dto],
    tolerance: f64,
) -> ProfileCurveDto {
    let start = path[0];
    let end = *path.last().unwrap_or(&start);
    if let Some(circle) = projected.circle {
        if circle.closed && point2_distance(start, end) <= tolerance {
            return ProfileCurveDto::Circle {
                entity_id,
                source_entity_ids: vec![entity_id],
                center: Point2Dto::new(circle.center.x, circle.center.y),
                radius: circle.radius,
            };
        }

        let mid = path[path.len() / 2];
        return ProfileCurveDto::Arc {
            entity_id,
            source_entity_ids: vec![entity_id],
            start,
            mid,
            end,
        };
    }
    if path.len() == 2 {
        return ProfileCurveDto::Line {
            entity_id,
            source_entity_ids: vec![entity_id],
            start,
            end,
        };
    }
    ProfileCurveDto::Polyline {
        entity_id,
        source_entity_ids: vec![entity_id],
        points: path.to_vec(),
    }
}

fn body_feature_kind(request: &BodyFeatureRequestDto) -> (FeatureKind, &'static str) {
    match request {
        BodyFeatureRequestDto::ExternalThread(_) => (FeatureKind::ExternalThread, "ExternalThread"),
        BodyFeatureRequestDto::Shell(_) => (FeatureKind::Shell, "Shell"),
        BodyFeatureRequestDto::MoveCopy(_) => (FeatureKind::MoveCopy, "MoveCopy"),
        BodyFeatureRequestDto::Mirror(_) => (FeatureKind::Mirror, "Mirror"),
        BodyFeatureRequestDto::RectangularPattern(_) => {
            (FeatureKind::RectangularPattern, "RectangularPattern")
        }
        BodyFeatureRequestDto::CircularPattern(_) => {
            (FeatureKind::CircularPattern, "CircularPattern")
        }
        BodyFeatureRequestDto::Combine(_) => (FeatureKind::Combine, "Combine"),
        BodyFeatureRequestDto::SplitBody(_) => (FeatureKind::SplitBody, "SplitBody"),
        BodyFeatureRequestDto::ImportStep(_) => (FeatureKind::ImportStep, "Import"),
    }
}

fn feature_changes_solid_topology(kind: FeatureKind) -> bool {
    !matches!(kind, FeatureKind::Sketch | FeatureKind::ConstructionPlane)
}

fn datum_source_reads_solid_topology(source: &DatumPlaneSourceDto) -> bool {
    match source {
        DatumPlaneSourceDto::Offset { reference, .. } => {
            matches!(reference, PlaneRef::PlanarFace { .. })
        }
        DatumPlaneSourceDto::Midplane { first, second } => matches!(
            (first, second),
            (PlaneRef::PlanarFace { .. }, _) | (_, PlaneRef::PlanarFace { .. })
        ),

        DatumPlaneSourceDto::AtAngle { .. } => true,
    }
}

fn resolve_datum_source(
    solids: &SolidDocument,
    planes: &[DatumPlaneDefinitionDto],
    active: &BTreeSet<FeatureId>,
    source: &mut DatumPlaneSourceDto,
) -> Result<PlaneBasis, SessionError> {
    let resolve = |reference: PlaneRef| -> Result<PlaneBasis, SessionError> {
        match reference {
            PlaneRef::OriginPlane { .. } => reference
                .origin_basis()
                .map_err(|_| SessionError::UnsupportedPlane),
            PlaneRef::PlanarFace { face_id } => solids.face_basis(face_id).ok_or_else(|| {
                SessionError::BrokenReference(format!(
                    "face {} no longer exists or is not planar",
                    face_id.0
                ))
            }),
            PlaneRef::DatumPlane { datum_id } => planes
                .iter()
                .find(|plane| plane.datum_id == datum_id && active.contains(&plane.feature_id))
                .map(|plane| plane.basis)
                .ok_or_else(|| {
                    SessionError::BrokenReference(format!(
                        "construction plane {} is missing or rolled back",
                        datum_id.0
                    ))
                }),
        }
    };

    construction_plane_basis(source, resolve, |body, edge| solids.edge_points(body, edge))
}

/// The same validated construction geometry serves history replay and native
/// previews. Callers resolve only references from their coherent model snapshot.
pub fn construction_plane_basis(
    source: &mut DatumPlaneSourceDto,
    resolve: impl Fn(PlaneRef) -> Result<PlaneBasis, SessionError>,
    edge_points: impl Fn(BodyId, limo_cad_core::EdgeId) -> Option<Vec<Point3Dto>>,
) -> Result<PlaneBasis, SessionError> {
    match source {
        DatumPlaneSourceDto::Offset {
            reference,
            distance,
        } => {
            if !distance.is_finite() {
                return Err(SessionError::Solid(
                    "offset distance must be finite".to_string(),
                ));
            }
            let mut basis = resolve(*reference)?;
            for axis in 0..3 {
                basis.origin[axis] += basis.normal[axis] * *distance;
            }
            Ok(basis)
        }
        DatumPlaneSourceDto::Midplane { first, second } => {
            let first_basis = resolve(*first)?;
            let second_basis = resolve(*second)?;
            if dot3(first_basis.normal, second_basis.normal).abs() < 1.0 - 1e-6 {
                return Err(SessionError::Solid(
                    "midplane references must be parallel".to_string(),
                ));
            }
            let delta = sub3(second_basis.origin, first_basis.origin);
            let distance = dot3(delta, first_basis.normal);
            if distance.abs() <= 1e-7 {
                return Err(SessionError::Solid(
                    "midplane references are coincident".to_string(),
                ));
            }
            let mut basis = first_basis;
            basis.origin = add3(
                first_basis.origin,
                scale3(first_basis.normal, distance * 0.5),
            );
            Ok(basis)
        }
        DatumPlaneSourceDto::AtAngle {
            reference,
            body_id,
            edge_id,
            angle_deg,
            axis_points,
        } => {
            if !angle_deg.is_finite() || angle_deg.abs() > 360.0 {
                return Err(SessionError::Solid(
                    "plane angle must be finite and between -360° and 360°".to_string(),
                ));
            }
            let basis = resolve(*reference)?;
            let points = edge_points(*body_id, *edge_id)
                .filter(|points| points.len() >= 2)
                .or_else(|| axis_points.map(|points| points.to_vec()))
                .ok_or_else(|| {
                    SessionError::BrokenReference(format!(
                        "axis edge {} on body {} is missing",
                        edge_id.0, body_id.0
                    ))
                })?;
            let start = point3_array(points[0]);
            let end = point3_array(*points.last().unwrap());
            let axis = normalize3(sub3(end, start)).ok_or_else(|| {
                SessionError::Solid("plane-at-angle axis edge has zero length".to_string())
            })?;
            if points.iter().any(|point| {
                let offset = sub3(point3_array(*point), start);
                length3(cross3(offset, axis)) > 1e-4
            }) {
                return Err(SessionError::Solid(
                    "plane-at-angle requires a straight edge".to_string(),
                ));
            }
            if [start, end]
                .iter()
                .any(|point| dot3(sub3(*point, basis.origin), basis.normal).abs() > 1e-4)
            {
                return Err(SessionError::Solid(
                    "the selected axis edge must lie on the reference plane".to_string(),
                ));
            }
            *axis_points = Some([points[0], *points.last().unwrap()]);
            let angle = angle_deg.to_radians();
            Ok(PlaneBasis {
                origin: add3(start, rotate_vector(sub3(basis.origin, start), axis, angle)),
                u: rotate_vector(basis.u, axis, angle),
                v: rotate_vector(basis.v, axis, angle),
                normal: rotate_vector(basis.normal, axis, angle),
            })
        }
    }
}

fn point3_array(point: Point3Dto) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(value: [f64; 3], scale: f64) -> [f64; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length3(value: [f64; 3]) -> f64 {
    dot3(value, value).sqrt()
}

fn normalize3(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = length3(value);
    (length > 1e-9 && length.is_finite()).then(|| scale3(value, 1.0 / length))
}

fn rotate_vector(value: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    let cosine = angle.cos();
    let sine = angle.sin();
    add3(
        add3(scale3(value, cosine), scale3(cross3(axis, value), sine)),
        scale3(axis, dot3(axis, value) * (1.0 - cosine)),
    )
}

fn max_numbered_name(sketches: &[FinishedSketch], prefix: &str) -> u32 {
    sketches
        .iter()
        .filter_map(|sketch| sketch.session.name().strip_prefix(prefix))
        .filter_map(|suffix| suffix.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
}

fn max_feature_number(document: &Document, prefix: &str) -> u32 {
    document
        .features()
        .features
        .iter()
        .filter_map(|feature| feature.name.strip_prefix(prefix))
        .filter_map(|suffix| suffix.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
}

fn cam_model_point_to_setup(point: [f64; 3], setup: &CamSetupDto) -> limo_cad_cam::Point3Dto {
    let delta = [
        point[0] - setup.wcs.origin.x,
        point[1] - setup.wcs.origin.y,
        point[2] - setup.wcs.origin.z,
    ];
    let project = |axis: [f64; 3]| delta[0] * axis[0] + delta[1] * axis[1] + delta[2] * axis[2];
    limo_cad_cam::Point3Dto::new(
        project(setup.wcs.x_axis),
        project(setup.wcs.y_axis),
        project(setup.wcs.z_axis),
    )
}

fn cam_direction_to_setup(direction: [f64; 3], setup: &CamSetupDto) -> [f64; 3] {
    let project =
        |axis: [f64; 3]| direction[0] * axis[0] + direction[1] * axis[1] + direction[2] * axis[2];
    [
        project(setup.wcs.x_axis),
        project(setup.wcs.y_axis),
        project(setup.wcs.z_axis),
    ]
}

fn resolve_cam_chain(
    source: CamChainSource,
    keys: &[String],
    reversed: bool,
    setup: &CamSetupDto,
    scene: &SolidSceneDto,
    sketches: &[SketchDto],
    planar: bool,
) -> Result<(Vec<limo_cad_cam::Point2Dto>, bool), String> {
    let chain = crate::edge_selection::resolve(
        scene,
        sketches,
        &crate::EdgeChainRequest {
            source: match source {
                CamChainSource::Model => crate::ChainSource::Model,
                CamChainSource::Sketch => crate::ChainSource::Sketch,
            },
            body_ids: setup.body_ids.clone(),
            normal: Some(setup.wcs.z_axis),
            keys: keys.to_vec(),
            mode: crate::ChainMode::Manual,
            reversed,
        },
    )?;
    if planar {
        let z = cam_model_point_to_setup(chain.points[0], setup).z;
        if chain.points.iter().any(|p| {
            (cam_model_point_to_setup(*p, setup).z - z).abs()
                > limo_cad_core::edge_chain::JOIN_TOLERANCE
        }) {
            return Err("The selected 2D boundary must lie in one setup-Z plane.".into());
        }
    }
    Ok((
        chain
            .points
            .into_iter()
            .map(|p| {
                let p = cam_model_point_to_setup(p, setup);
                limo_cad_cam::Point2Dto::new(p.x, p.y)
            })
            .collect(),
        chain.closed,
    ))
}

pub fn resolve_cam_hole(
    reference: &str,
    hole: &mut CamHoleDto,
    setup: &CamSetupDto,
    scene: &SolidSceneDto,
) -> Result<(), String> {
    let (body_text, face_text) = reference
        .split_once(':')
        .ok_or_else(|| format!("hole reference '{reference}' is malformed."))?;
    let body_id = body_text
        .parse::<u64>()
        .map_err(|_| format!("hole reference '{reference}' has an invalid body id."))?;
    let face_id = face_text
        .parse::<u64>()
        .map_err(|_| format!("hole reference '{reference}' has an invalid face id."))?;
    let body = scene
        .bodies
        .iter()
        .find(|body| body.id.0 == body_id)
        .ok_or_else(|| format!("referenced hole body {body_id} no longer exists."))?;
    let face = body
        .faces
        .iter()
        .find(|face| face.id.0 == face_id)
        .ok_or_else(|| format!("referenced cylindrical face {reference} no longer exists."))?;
    let cylinder = face
        .cylinder
        .ok_or_else(|| format!("referenced face {reference} is no longer cylindrical."))?;
    let center = cam_model_point_to_setup(
        [cylinder.origin.x, cylinder.origin.y, cylinder.origin.z],
        setup,
    );
    let axis = cam_direction_to_setup([cylinder.axis.x, cylinder.axis.y, cylinder.axis.z], setup);
    let axis_length = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if !axis_length.is_finite()
        || axis_length <= 1.0e-9
        || (axis[2] / axis_length).abs() < 1.0 - 1.0e-6
    {
        return Err(format!(
            "referenced face {reference} is no longer aligned with setup Z."
        ));
    }
    let mut top = f64::NEG_INFINITY;
    let mut bottom = f64::INFINITY;
    let start = face.first_index as usize;
    let end = start.saturating_add(face.index_count as usize);
    for &vertex in body.mesh.indices.get(start..end).unwrap_or_default() {
        let base = vertex as usize * 3;
        let Some((&x, rest)) = body
            .mesh
            .positions
            .get(base)
            .zip(body.mesh.positions.get(base + 1..))
        else {
            continue;
        };
        let Some((&y, rest)) = rest.split_first() else {
            continue;
        };
        let Some(&z) = rest.first() else {
            continue;
        };
        let point = cam_model_point_to_setup([f64::from(x), f64::from(y), f64::from(z)], setup);
        top = top.max(point.z);
        bottom = bottom.min(point.z);
    }
    if !top.is_finite() || !bottom.is_finite() || top <= bottom + 1.0e-9 {
        return Err(format!(
            "referenced face {reference} has no trustworthy axial span."
        ));
    }
    hole.point = limo_cad_cam::Point2Dto::new(center.x, center.y);
    hole.top_z = top;
    hole.bottom_z = bottom;
    hole.axis = [
        axis[0] / axis_length,
        axis[1] / axis_length,
        axis[2] / axis_length,
    ];
    Ok(())
}

fn cam_height_expressions_use_selection(expressions: &CamOperationHeightExpressionsDto) -> bool {
    [
        &expressions.clearance,
        &expressions.retract,
        &expressions.feed,
        &expressions.top,
    ]
    .into_iter()
    .chain(expressions.bottom.iter())
    .any(|expression| expression.reference == CamHeightReferenceDto::Selection)
}

fn cam_selection_reference_z(
    operation: &CamOperationDto,
    setup: &CamSetupDto,
    scene: &SolidSceneDto,
    sketches: &[SketchDto],
    label: &str,
) -> Result<f64, SessionError> {
    if matches!(operation, CamOperationDto::Chamfer2d { additional_chains, .. } if !additional_chains.is_empty())
    {
        return operation
            .chamfer_chains()
            .into_iter()
            .map(|chain| {
                cam_selection_reference_z(
                    &operation.with_chamfer_chain(chain),
                    setup,
                    scene,
                    sketches,
                    label,
                )
            })
            .try_fold(f64::NEG_INFINITY, |highest, z| z.map(|z| highest.max(z)));
    }
    let reference = match operation {
        CamOperationDto::Contour2d { chain_ref, .. }
        | CamOperationDto::Pocket2d { chain_ref, .. }
        | CamOperationDto::Chamfer2d { chain_ref, .. } => chain_ref.as_ref(),
        _ => None,
    }
    .ok_or_else(|| {
        SessionError::Solid(format!(
            "Cannot regenerate operation '{label}': its Selection height has no persisted geometry reference. Reselect its sketch loop."
        ))
    })?;
    if reference.source == CamChainSource::Model {
        let chain = crate::edge_selection::resolve(
            scene,
            sketches,
            &crate::EdgeChainRequest {
                source: crate::ChainSource::Model,
                body_ids: setup.body_ids.clone(),
                normal: Some(setup.wcs.z_axis),
                keys: reference.keys.clone(),
                mode: crate::ChainMode::Manual,
                reversed: false,
            },
        )
        .map_err(|e| SessionError::Solid(format!("Cannot regenerate operation '{label}': {e}")))?;
        let levels: Vec<_> = chain
            .points
            .iter()
            .map(|p| cam_model_point_to_setup(*p, setup).z)
            .collect();
        let z = levels[0];
        if levels
            .iter()
            .any(|v| (v - z).abs() > limo_cad_core::edge_chain::JOIN_TOLERANCE)
        {
            return Err(SessionError::Solid(format!("Cannot regenerate operation '{label}': Selection height requires a chain in one setup-Z plane.")));
        }
        return Ok(z);
    }
    let key = reference.keys.first().ok_or_else(|| {
        SessionError::Solid(format!(
            "Cannot regenerate operation '{label}': its Selection height has an empty sketch reference."
        ))
    })?;
    let value = key.strip_prefix("sketch:").ok_or_else(|| {
        SessionError::Solid(format!(
            "Cannot regenerate operation '{label}': its Selection height reference is malformed."
        ))
    })?;
    let (sketch_name, entity_id) = value.rsplit_once(':').ok_or_else(|| {
        SessionError::Solid(format!(
            "Cannot regenerate operation '{label}': its Selection height reference is malformed."
        ))
    })?;
    entity_id.parse::<u64>().map_err(|_| {
        SessionError::Solid(format!(
            "Cannot regenerate operation '{label}': its Selection height entity id is malformed."
        ))
    })?;
    let sketch = sketches
        .iter()
        .find(|sketch| sketch.name == sketch_name)
        .ok_or_else(|| {
            SessionError::Solid(format!(
                "Cannot regenerate operation '{label}': referenced sketch '{sketch_name}' no longer exists. Reselect its geometry."
            ))
        })?;
    let point = cam_model_point_to_setup(sketch.basis.origin, setup);
    Ok(point.z)
}

fn cam_apply_resolved_heights(
    operation: &mut CamOperationDto,
    bottom: Option<f64>,
    top: f64,
    feed: f64,
    retract: f64,
    clearance: f64,
) -> Result<(), SessionError> {
    let label = operation.name().to_string();
    let required_bottom = || {
        bottom.ok_or_else(|| {
            SessionError::Solid(format!(
                "Cannot regenerate operation '{}': resolved Bottom height is missing.",
                label
            ))
        })
    };
    match operation {
        CamOperationDto::Adaptive3d {
            top_z,
            bottom_z,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        }
        | CamOperationDto::Flat3d {
            top_z,
            bottom_z,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        }
        | CamOperationDto::Contour2d {
            top_z,
            bottom_z,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        }
        | CamOperationDto::Pocket2d {
            top_z,
            bottom_z,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        }
        | CamOperationDto::Thread {
            top_z,
            bottom_z,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        }
        | CamOperationDto::Drill {
            top_z,
            bottom_z,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        } => {
            *top_z = top;
            *bottom_z = required_bottom()?;
            *feed_height_z = feed;
            *retract_z = retract;
            *clearance_z = clearance;
        }
        CamOperationDto::Face {
            top_z,
            target_z,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        } => {
            *top_z = top;
            *target_z = required_bottom()?;
            *feed_height_z = feed;
            *retract_z = retract;
            *clearance_z = clearance;
        }
        CamOperationDto::Chamfer2d {
            top_z,
            modeled_chamfer,
            additional_chains,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        } => {
            if modeled_chamfer.is_none() {
                *top_z = top;
            }
            for chain in additional_chains {
                if chain.modeled_chamfer.is_none() {
                    chain.top_z = top;
                }
            }
            *feed_height_z = feed;
            *retract_z = retract;
            *clearance_z = clearance;
        }
    }
    Ok(())
}

impl Default for SketchManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Sketch-curve segment ids are `curve id * SEGMENT_ID_STRIDE + piece index`
/// (see `push_polyline_segments` and `ordered_profile_curves`).
const SEGMENT_ID_STRIDE: u64 = 1_000;

/// Reserved id range for projected support-face boundary edges. Keeping these
/// ids above every authored entity id makes the noding dedupe (`min` of two
/// coincident segment ids) preserve authored provenance when a projected edge
/// and a drawn curve overlap.
const PROJECTED_EDGE_ID_BASE: u64 = 1 << 40;

pub(crate) fn profile_catalog_item(
    sketch: &SketchDto,
    feature_id: FeatureId,
) -> ProfileCatalogItemDto {
    const PROFILE_TOLERANCE: f64 = 1e-5;

    const CONSUMED_LINE_TOLERANCE: f64 = 1e-3;
    let mut segments = Vec::new();
    let mut projected_segments = BTreeSet::new();
    let mut lines = Vec::new();
    let mut path_curves = Vec::new();
    let mut reference_points = Vec::new();
    let consumed_trim_carriers = consumed_trim_carrier_ids(sketch, CONSUMED_LINE_TOLERANCE);
    for entity in &sketch.entities {
        match entity {
            crate::dto::EntityDto::Line { id, start, end, .. } => {
                let line = SketchLineDto {
                    entity_id: id.0,
                    start: Point2Dto::new(start.x, start.y),
                    end: Point2Dto::new(end.x, end.y),
                };
                lines.push(line.clone());
                reference_points.push(SketchReferencePointDto {
                    entity_id: id.0,
                    point: SketchPointKindDto::Start,
                    position: line.start,
                });
                reference_points.push(SketchReferencePointDto {
                    entity_id: id.0,
                    point: SketchPointKindDto::End,
                    position: line.end,
                });
                path_curves.push(SketchPathCurveDto::Line {
                    entity_id: line.entity_id,
                    start: line.start,
                    end: line.end,
                });
                let a = Point2Dto::new(start.x, start.y);
                let b = Point2Dto::new(end.x, end.y);

                if !consumed_trim_carriers.contains(&id.0) {
                    segments.push(Segment2 {
                        id: id.0 * 1_000,
                        a,
                        b,
                    });
                }
            }
            crate::dto::EntityDto::Circle {
                id, center, radius, ..
            } => {
                reference_points.push(SketchReferencePointDto {
                    entity_id: id.0,
                    point: SketchPointKindDto::Center,
                    position: Point2Dto::new(center.x, center.y),
                });
                path_curves.push(SketchPathCurveDto::Circle {
                    entity_id: id.0,
                    center: Point2Dto::new(center.x, center.y),
                    radius: *radius,
                });
                let points = (0..64)
                    .map(|index| {
                        let angle = TAU * index as f64 / 64.0;
                        Point2Dto::new(
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        )
                    })
                    .collect::<Vec<_>>();
                push_polyline_segments(&mut segments, id.0, &points, true);
            }
            crate::dto::EntityDto::Arc {
                id,
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => {
                let raw = end_angle - start_angle;
                let sweep = if raw.abs() >= TAU - 1e-8 {
                    TAU
                } else {
                    raw.rem_euclid(TAU)
                };
                let steps = ((sweep / TAU) * 64.0).ceil().max(8.0) as usize;
                let points = (0..=steps)
                    .map(|index| {
                        let angle = start_angle + sweep * index as f64 / steps as f64;
                        Point2Dto::new(
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        )
                    })
                    .collect::<Vec<_>>();
                reference_points.push(SketchReferencePointDto {
                    entity_id: id.0,
                    point: SketchPointKindDto::Center,
                    position: Point2Dto::new(center.x, center.y),
                });
                if let Some(start) = points.first().copied() {
                    reference_points.push(SketchReferencePointDto {
                        entity_id: id.0,
                        point: SketchPointKindDto::Start,
                        position: start,
                    });
                }
                if let Some(end) = points.last().copied() {
                    reference_points.push(SketchReferencePointDto {
                        entity_id: id.0,
                        point: SketchPointKindDto::End,
                        position: end,
                    });
                }
                if let (Some(start), Some(mid), Some(end)) = (
                    points.first().copied(),
                    points.get(points.len() / 2).copied(),
                    points.last().copied(),
                ) {
                    path_curves.push(SketchPathCurveDto::Arc {
                        entity_id: id.0,
                        start,
                        mid,
                        end,
                    });
                }
                push_polyline_segments(&mut segments, id.0, &points, false);
            }
            crate::dto::EntityDto::Spline {
                id,
                points: fit_points,
                tessellation,
                ..
            } => {
                reference_points.extend(fit_points.iter().enumerate().map(|(index, point)| {
                    SketchReferencePointDto {
                        entity_id: id.0,
                        point: SketchPointKindDto::FitPoint {
                            index: index as u32,
                        },
                        position: Point2Dto::new(point.x, point.y),
                    }
                }));
                let points = tessellation
                    .iter()
                    .map(|point| Point2Dto::new(point.x, point.y))
                    .collect::<Vec<_>>();
                path_curves.push(SketchPathCurveDto::Spline {
                    entity_id: id.0,
                    points: points.clone(),
                });
                push_polyline_segments(&mut segments, id.0, &points, false);
            }
            crate::dto::EntityDto::Point { id, position, .. } => {
                reference_points.push(SketchReferencePointDto {
                    entity_id: id.0,
                    point: SketchPointKindDto::Point,
                    position: Point2Dto::new(position.x, position.y),
                });
            }
        }
    }

    debug_assert!(
        sketch
            .entities
            .iter()
            .map(|entity| entity.id().0)
            .max()
            .unwrap_or(0)
            < PROJECTED_EDGE_ID_BASE,
        "authored entity ids must stay below the reserved projected id range"
    );
    let contacts = sketch
        .entities
        .iter()
        .flat_map(|entity| match entity {
            crate::dto::EntityDto::Point { position, .. } => vec![*position],
            crate::dto::EntityDto::Line { start, end, .. } => vec![*start, *end],
            _ => vec![],
        })
        .collect::<Vec<_>>();
    for edge in sketch.projected_edges.iter() {
        debug_assert!(
            edge.id >= PROJECTED_EDGE_ID_BASE,
            "projected boundary ids must use the reserved range"
        );
        let projected_id = edge.id;
        for (piece, pair) in edge
            .profile_points(&contacts, PROFILE_TOLERANCE)
            .windows(2)
            .enumerate()
        {
            let a = Point2Dto::new(pair[0].x, pair[0].y);
            let b = Point2Dto::new(pair[1].x, pair[1].y);
            if point2_distance(a, b) <= PROFILE_TOLERANCE {
                continue;
            }
            let id = projected_id * SEGMENT_ID_STRIDE + piece as u64;
            projected_segments.insert(id);
            segments.push(Segment2 { id, a, b });
        }
    }

    let (loops, profile_error) = if segments.is_empty() {
        (Vec::new(), None)
    } else {
        match extract_bounded_faces(&segments, PROFILE_TOLERANCE, &projected_segments) {
            Ok(faces) => (
                faces
                    .into_iter()
                    .filter(|face| face.authored_edges > 0)
                    .map(|face| face.points)
                    .collect::<Vec<_>>(),
                None,
            ),
            Err(error) => (Vec::new(), Some(error.to_string())),
        }
    };
    let mut profiles = loops
        .into_iter()
        .enumerate()
        .map(|(index, points)| ProfileLoopDto {
            index: index as u32,
            area: polygon_area(&points).abs(),
            parent_index: None,
            nesting_depth: 0,
            curves: canonicalize_profile_curves(
                &ordered_profile_curves(sketch, &segments, &points, PROFILE_TOLERANCE),
                PROFILE_TOLERANCE,
            ),
            points,
        })
        .collect::<Vec<_>>();
    classify_profile_nesting(&mut profiles, PROFILE_TOLERANCE);
    ProfileCatalogItemDto {
        sketch_name: sketch.name.clone(),
        feature_id,
        basis: sketch.basis,
        profiles,
        profile_error,
        lines,
        path_curves,
        reference_points,
    }
}

/// A sub-micron line is not automatically disposable: users can deliberately
/// model tiny geometry. Suppress it only when both endpoints are owned by
/// corner modifiers and the line has actually reached the limiting condition.
fn consumed_trim_carrier_ids(sketch: &SketchDto, tolerance: f64) -> BTreeSet<u64> {
    let arc_tangencies = sketch
        .constraints
        .iter()
        .filter_map(|constraint| match constraint.constraint {
            Constraint::Tangent { a, b } => Some((a.0, b.0)),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    sketch
        .entities
        .iter()
        .filter_map(|entity| {
            let crate::dto::EntityDto::Line {
                id,
                start_id,
                end_id,
                start,
                end,
                ..
            } = entity
            else {
                return None;
            };
            if start.distance(*end) > tolerance {
                return None;
            }
            let endpoint_is_trimmed = |point: EntityId| {
                sketch
                    .constraints
                    .iter()
                    .any(|constraint| match constraint.constraint {
                        Constraint::ArcEndpointCoincident {
                            point: owner, arc, ..
                        } if owner == point => {
                            arc_tangencies.contains(&(id.0, arc.0))
                                || arc_tangencies.contains(&(arc.0, id.0))
                        }
                        Constraint::EqualDistance { a, b, .. } => a == point || b == point,
                        _ => false,
                    })
            };
            (endpoint_is_trimmed(*start_id) && endpoint_is_trimmed(*end_id)).then_some(id.0)
        })
        .collect()
}

fn classify_profile_nesting(profiles: &mut [ProfileLoopDto], tolerance: f64) {
    let parents = profiles
        .iter()
        .map(|profile| {
            let sample = profile.points.first().copied()?;
            profiles
                .iter()
                .filter(|candidate| candidate.area > profile.area + 1e-8)
                .filter(|candidate| point_in_polygon_strict(sample, &candidate.points, tolerance))
                .min_by(|a, b| a.area.total_cmp(&b.area))
                .map(|candidate| candidate.index)
        })
        .collect::<Vec<_>>();
    for (profile, parent) in profiles.iter_mut().zip(parents) {
        profile.parent_index = parent;
    }
    let parent_map = profiles
        .iter()
        .map(|profile| (profile.index, profile.parent_index))
        .collect::<std::collections::HashMap<_, _>>();
    for profile in profiles {
        let mut depth = 0;
        let mut current = profile.parent_index;
        let mut visited = BTreeSet::new();
        while let Some(parent) = current {
            if !visited.insert(parent) {
                break;
            }
            depth += 1;
            current = parent_map.get(&parent).copied().flatten();
        }
        profile.nesting_depth = depth;
    }
}

fn point_in_polygon_strict(point: Point2Dto, polygon: &[Point2Dto], tolerance: f64) -> bool {
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let length2 = dx * dx + dy * dy;
        if length2 <= tolerance * tolerance {
            continue;
        }
        let t = (((point.x - a.x) * dx + (point.y - a.y) * dy) / length2).clamp(0.0, 1.0);
        let closest = Point2Dto::new(a.x + t * dx, a.y + t * dy);
        if point2_distance(point, closest) <= tolerance {
            return false;
        }
    }

    let mut inside = false;
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        let crosses = (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x;
        if crosses {
            inside = !inside;
        }
    }
    inside
}

fn push_polyline_segments(
    segments: &mut Vec<Segment2>,
    entity_id: u64,
    points: &[Point2Dto],
    close: bool,
) {
    for (index, pair) in points.windows(2).enumerate() {
        segments.push(Segment2 {
            id: entity_id * 1_000 + index as u64,
            a: pair[0],
            b: pair[1],
        });
    }
    if close && points.len() > 2 {
        segments.push(Segment2 {
            id: entity_id * 1_000 + points.len() as u64,
            a: *points.last().unwrap(),
            b: points[0],
        });
    }
}

fn polygon_area(points: &[Point2Dto]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f64>()
        * 0.5
}

fn point2_distance(a: Point2Dto, b: Point2Dto) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn segment_length_squared(segment: &Segment2) -> f64 {
    (segment.b.x - segment.a.x).powi(2) + (segment.b.y - segment.a.y).powi(2)
}

fn segment_contains_profile_edge(
    segment: &Segment2,
    a: Point2Dto,
    b: Point2Dto,
    tolerance: f64,
) -> bool {
    let dx = segment.b.x - segment.a.x;
    let dy = segment.b.y - segment.a.y;
    let length2 = dx * dx + dy * dy;
    if length2 <= tolerance * tolerance {
        return false;
    }
    let length = length2.sqrt();
    [a, b].into_iter().all(|point| {
        let parameter = ((point.x - segment.a.x) * dx + (point.y - segment.a.y) * dy) / length2;
        let parameter_tolerance = tolerance / length;
        if parameter < -parameter_tolerance || parameter > 1.0 + parameter_tolerance {
            return false;
        }
        let projected = Point2Dto::new(
            segment.a.x + parameter.clamp(0.0, 1.0) * dx,
            segment.a.y + parameter.clamp(0.0, 1.0) * dy,
        );
        point2_distance(point, projected) <= tolerance
    })
}

/// Recover the source sketch entity for each tessellated loop edge, then
/// collapse consecutive samples back into one ordered analytic curve. The
/// polygon remains available as a compatibility fallback, but OCCT receives
/// one arc rather than the 8–64 chords used to discover the profile.
fn ordered_profile_curves(
    sketch: &SketchDto,
    segments: &[Segment2],
    points: &[Point2Dto],
    tolerance: f64,
) -> Vec<ProfileCurveDto> {
    if points.len() < 3 {
        return Vec::new();
    }
    let source_ids = points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| {
            segments
                .iter()
                .filter(|segment| segment_contains_profile_edge(segment, a, b, tolerance))
                .max_by(|left, right| {
                    segment_length_squared(left)
                        .total_cmp(&segment_length_squared(right))
                        .then_with(|| right.id.cmp(&left.id))
                })
                .map(|segment| segment.id / 1_000)
        })
        .collect::<Option<Vec<_>>>();
    let Some(source_ids) = source_ids else {
        return Vec::new();
    };

    let start_edge = (0..source_ids.len())
        .find(|index| {
            source_ids[*index] != source_ids[(*index + source_ids.len() - 1) % source_ids.len()]
        })
        .unwrap_or(0);
    let mut groups: Vec<(u64, Vec<Point2Dto>)> = Vec::new();
    for step in 0..source_ids.len() {
        let edge = (start_edge + step) % source_ids.len();
        let source = source_ids[edge];
        let a = points[edge];
        let b = points[(edge + 1) % points.len()];
        if groups.last().is_none_or(|(id, _)| *id != source) {
            groups.push((source, vec![a, b]));
        } else {
            groups.last_mut().unwrap().1.push(b);
        }
    }

    groups
        .into_iter()
        .filter_map(|(entity_id, path)| {
            let start = path[0];
            let end = *path.last()?;

            if let Some(projected) = sketch
                .projected_edges
                .iter()
                .find(|edge| edge.id == entity_id)
            {
                return Some(projected_profile_curve(
                    projected, entity_id, &path, tolerance,
                ));
            }
            let entity = sketch.entities.iter().find(|entity| match entity {
                crate::dto::EntityDto::Point { id, .. }
                | crate::dto::EntityDto::Line { id, .. }
                | crate::dto::EntityDto::Circle { id, .. }
                | crate::dto::EntityDto::Arc { id, .. }
                | crate::dto::EntityDto::Spline { id, .. } => id.0 == entity_id,
            })?;
            Some(match entity {
                crate::dto::EntityDto::Line { .. } => ProfileCurveDto::Line {
                    entity_id,
                    source_entity_ids: vec![entity_id],
                    start,
                    end,
                },
                crate::dto::EntityDto::Arc { center, radius, .. }
                    if point2_distance(start, end) <= tolerance =>
                {
                    ProfileCurveDto::Circle {
                        entity_id,
                        source_entity_ids: vec![entity_id],
                        center: Point2Dto::new(center.x, center.y),
                        radius: *radius,
                    }
                }
                crate::dto::EntityDto::Arc { .. } => ProfileCurveDto::Arc {
                    entity_id,
                    source_entity_ids: vec![entity_id],
                    start,
                    mid: path[path.len() / 2],
                    end,
                },
                crate::dto::EntityDto::Circle { center, radius, .. }
                    if point2_distance(start, end) <= tolerance =>
                {
                    ProfileCurveDto::Circle {
                        entity_id,
                        source_entity_ids: vec![entity_id],
                        center: Point2Dto::new(center.x, center.y),
                        radius: *radius,
                    }
                }

                crate::dto::EntityDto::Circle { .. } => ProfileCurveDto::Arc {
                    entity_id,
                    source_entity_ids: vec![entity_id],
                    start,
                    mid: path[path.len() / 2],
                    end,
                },
                crate::dto::EntityDto::Spline { .. } => ProfileCurveDto::Polyline {
                    entity_id,
                    source_entity_ids: vec![entity_id],
                    points: path,
                },
                crate::dto::EntityDto::Point { .. } => return None,
            })
        })
        .collect()
}

#[cfg(test)]
mod project_tests {
    use super::*;

    /// Issue #151, review finding 6: a project saved before center handles
    /// existed must still give its circles a selectable center on load.
    #[test]
    fn loading_a_sketch_saved_without_center_handles_restores_them() {
        let plane = PlaneRef::OriginPlane {
            plane: limo_cad_core::OriginPlane::Xy,
        };
        let center = crate::geometry::Vec2::new(12.0, 8.0);
        let mut session = SketchSession::new("Legacy", plane, plane.basis().unwrap(), false);
        let circle = session
            .add_circle(
                crate::dto::CircleMode::CenterDiameter,
                center,
                crate::geometry::Vec2::new(16.0, 8.0),
            )
            .unwrap()
            .entities[0];

        let handle = session
            .dto()
            .constraints
            .iter()
            .find_map(|constraint| match constraint.constraint {
                crate::constraint::Constraint::CenterCoincident { point, curve }
                    if curve == circle =>
                {
                    Some(point)
                }
                _ => None,
            })
            .expect("the drawn circle owns a center handle");
        session.delete_entities(&[handle]).unwrap();
        assert!(!session.dto().constraints.iter().any(|constraint| matches!(
            constraint.constraint,
            crate::constraint::Constraint::CenterCoincident { .. }
        )));

        let reloaded =
            SketchSession::from_project_state(session.project_state(limo_cad_core::FeatureId(1)))
                .unwrap();
        let dto = reloaded.dto();
        let handles: Vec<_> = dto
            .constraints
            .iter()
            .filter_map(|constraint| match constraint.constraint {
                crate::constraint::Constraint::CenterCoincident { point, curve }
                    if curve == circle =>
                {
                    Some(point)
                }
                _ => None,
            })
            .collect();
        assert_eq!(handles.len(), 1, "the loaded circle gains a center handle");
        assert!(
            dto.entities.iter().any(|entity| matches!(
                entity,
                crate::dto::EntityDto::Point { position, .. }
                    if position.distance(center) < 1e-6
            )),
            "the restored handle sits on the circle center"
        );
    }

    #[test]
    fn empty_edge_refinements_report_edges_without_mutating_the_document() {
        let mut manager = SketchManager::new();
        let original = manager.export_project_model().unwrap();
        let fillet = SolidFilletRequest {
            body_id: limo_cad_core::BodyId(1),
            edge_ids: vec![],
            radius: 1.0,
            tangent_chain: false,
        };
        let chamfer = SolidChamferRequest {
            body_id: limo_cad_core::BodyId(1),
            edge_ids: vec![],
            distance: 1.0,
            tangent_chain: false,
        };
        for rejected in [
            manager.prepare_solid_fillet(fillet.clone()),
            manager.prepare_solid_chamfer(chamfer.clone()),
            manager.prepare_edit_solid_fillet(EditSolidFilletRequest {
                feature_id: limo_cad_core::FeatureId(1),
                fillet,
            }),
            manager.prepare_edit_solid_chamfer(EditSolidChamferRequest {
                feature_id: limo_cad_core::FeatureId(1),
                chamfer,
            }),
        ] {
            assert!(
                matches!(rejected, Err(SessionError::Solid(ref message))
                if message == "select at least one edge"),
                "{rejected:?}"
            );
            assert_eq!(manager.export_project_model().unwrap(), original);
        }
        assert_eq!(
            manager.document.alloc_feature_id(),
            limo_cad_core::FeatureId(1),
            "rejected selection must not consume a feature identity"
        );
    }
    use crate::{
        DrawingAnnotationDto, DrawingDocumentDto, DrawingEdgeEndpoint, DrawingLineRefDto,
        DrawingLinearDimensionMode, DrawingProjectionMethod, DrawingSheetDto, DrawingSheetFormat,
        DrawingSheetOrientation, DrawingStandard, DrawingTitleBlockDto, DrawingToleranceNoteDto,
        DrawingTolerancePreset, DrawingTopologyAnchorRefDto, DrawingViewAlignment, DrawingViewDto,
        DrawingViewKind,
    };
    use limo_cad_cam::{
        CamChainRefDto, CamChainSource, CamHeightExpressionDto, CamHeightReferenceDto, CamHoleDto,
        CamOperationDto, CamOperationHeightExpressionsDto, CamPostConfigDto, CamSetupDto,
        CamToolDto, CamToolKind, CamUnits, CompensationMode, ContourCompensation, CoolantMode,
        CuttingParametersDto, DrillCycle, MillingDirection, Point2Dto as CamPoint2Dto,
        Point3Dto as CamPoint3Dto, Rect2Dto as CamRect2Dto, StockBoxDto, WcsOriginSpecDto,
        WorkCoordinateSystemDto, WorkOffset,
    };
    use limo_cad_core::{BodyId, DimensionStyle, OriginPlane};
    use limo_cad_solid::{
        CylindricalSurfaceDto, ExtrudeExtent, ExtrudeOperation, HoleExtent, HoleStyle,
        ImportStepRequest, KernelBodyDto, KernelCurveDto, KernelEdgeDto, KernelFaceDto,
        KernelJobDto, KernelSceneDto, LoftRequest, PlanarFaceSignatureDto, Point3Dto,
        ProfileRefDto, ReorderFeatureRequest, RibRequest, SweepRequest,
    };

    fn raw_body(body_id: BodyId, basis: limo_cad_core::PlaneBasis) -> KernelBodyDto {
        KernelBodyDto {
            topology_signature: String::new(),
            display_warnings: Vec::new(),
            body_id,
            positions: vec![0.0, 0.0, 0.0, 20.0, 0.0, 0.0, 0.0, 10.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![KernelFaceDto {
                linear_seam_edge_keys: Vec::new(),
                outer_shell: None,
                key: "face:0".to_string(),
                first_index: 0,
                index_count: 3,
                plane: Some(basis),
                signature: Some(PlanarFaceSignatureDto {
                    centroid: Point3Dto {
                        x: 20.0 / 3.0,
                        y: 10.0 / 3.0,
                        z: 0.0,
                    },
                    normal: Point3Dto::from(basis.normal),
                    area: 100.0,
                    perimeter: 30.0 + 500.0_f64.sqrt(),
                    wire_count: 1,
                    edge_count: 3,
                }),
                cylinder: None,
                edge_keys: Vec::new(),
                cone: None,
            }],
            edges: vec![
                KernelEdgeDto {
                    key: "edge:0".to_string(),
                    points: vec![
                        Point3Dto::from([0.0, 0.0, 0.0]),
                        Point3Dto::from([20.0, 0.0, 0.0]),
                    ],
                    circle: None,
                    refinable: true,
                },
                KernelEdgeDto {
                    key: "edge:1".to_string(),
                    points: vec![
                        Point3Dto::from([20.0, 0.0, 0.0]),
                        Point3Dto::from([0.0, 10.0, 0.0]),
                    ],
                    circle: None,
                    refinable: true,
                },
            ],
        }
    }

    fn result_body_ids(job: &KernelJobDto) -> &[BodyId] {
        match job {
            KernelJobDto::Extrude(job) => &job.result_body_ids,
            KernelJobDto::Revolve(job) => &job.result_body_ids,
            KernelJobDto::Sweep(job) => &job.result_body_ids,
            KernelJobDto::Loft(job) => &job.result_body_ids,
            KernelJobDto::Rib(job) => &job.result_body_ids,
            KernelJobDto::Fillet(job) => std::slice::from_ref(&job.target_body_id),
            KernelJobDto::Chamfer(job) => std::slice::from_ref(&job.target_body_id),
            KernelJobDto::Hole(job) => std::slice::from_ref(&job.target_body_id),
            KernelJobDto::ExternalThread(job) => std::slice::from_ref(&job.target_body_id),
            KernelJobDto::Shell(job) => std::slice::from_ref(&job.target_body_id),
            KernelJobDto::Transform(job) => &job.result_body_ids,
            KernelJobDto::Combine(job) => std::slice::from_ref(&job.target_body_id),
            KernelJobDto::SplitBody(job) => std::slice::from_ref(&job.new_body_id),
            KernelJobDto::ImportStep(job) => std::slice::from_ref(&job.result_body_id),
        }
    }

    fn commit_plan(
        manager: &mut SketchManager,
        plan: RecomputePlanDto,
        basis: limo_cad_core::PlaneBasis,
    ) {
        let ids = plan
            .jobs
            .iter()
            .flat_map(result_body_ids)
            .copied()
            .collect::<BTreeSet<_>>();
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: ids.into_iter().map(|id| raw_body(id, basis)).collect(),
                    errors: Vec::new(),
                },
            })
            .unwrap();
    }

    #[test]
    fn project_roundtrip_replays_feature_history_and_stable_body_ids() {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let basis = plane.origin_basis().unwrap();
        manager.begin_sketch(plane).unwrap();
        manager
            .add_rectangle_locked(LockedRectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                anchor: crate::Vec2::new(0.0, 0.0),
                width_mm: Some(20.0),
                height_mm: Some(10.0),
                width_text: Some("20".to_string()),
                height_text: Some("10".to_string()),
                corner_hint: crate::Vec2::new(20.0, 10.0),
                ctrl_held: false,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 15.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let body_id = result_body_ids(&plan.jobs[0])[0];
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(body_id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();

        let json = manager.export_project_model().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["format"], PROJECT_FORMAT);
        assert_eq!(parsed["schema_version"], PROJECT_SCHEMA_VERSION);
        assert!(parsed.get("scene").is_none());

        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        assert_eq!(replay.jobs.len(), 1);
        assert_eq!(result_body_ids(&replay.jobs[0]), &[body_id]);
        let update = loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(body_id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();
        assert_eq!(update.document.features.len(), 2);
        assert_eq!(loaded.finished_sketches().len(), 1);
        assert_eq!(update.scene.bodies[0].id, body_id);
        assert!(loaded
            .export_project_model()
            .unwrap()
            .contains("\"Sketch1\""));
    }

    #[test]
    fn named_layout_identity_is_owned_atomic_and_retained_across_rename_and_load() {
        let mut manager = SketchManager::new();
        let view = NamedViewConfigurationDto {
            id: None,
            name: "Print layout".into(),
            camera: crate::dto::ViewCameraDto {
                position: [30., -40., 20.],
                target: [0.; 3],
                up: [0., 0., 1.],
            },
            visible_body_ids: Vec::new(),
            part_offsets: Vec::new(),
            occurrence_offsets: Vec::new(),
            print_layout: true,
            print_bed: Default::default(),
        };
        // A legacy layout remains unassigned through reads and project load.
        manager.named_views = vec![view.clone()];
        let legacy = manager.export_project_model().unwrap();
        assert_eq!(manager.named_views().views[0].id, None);
        let basis = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .origin_basis()
        .unwrap();
        let mut loaded = SketchManager::new();
        let plan = loaded.prepare_load_project(legacy).unwrap();
        commit_plan(&mut loaded, plan, basis);
        assert_eq!(loaded.named_views().views[0].id, None);

        let stored = loaded.upsert_named_view(view.clone()).unwrap();
        let id = stored.views[0].id.clone().unwrap();
        assert_eq!(uuid::Uuid::parse_str(&id).unwrap().to_string(), id);
        loaded
            .rename_named_view(view.name.clone(), "Auger horizontal".into())
            .unwrap();
        assert_eq!(loaded.named_views().views[0].id.as_ref(), Some(&id));

        let before = loaded.export_project_model().unwrap();
        let mut replacement = loaded.named_views().views[0].clone();
        replacement.id = Some(uuid::Uuid::new_v4().to_string());
        assert!(loaded.upsert_named_view(replacement).is_err());
        let mut duplicate = loaded.named_views().views[0].clone();
        duplicate.name = "Duplicate identity".into();
        assert!(loaded
            .set_named_views(vec![loaded.named_views().views[0].clone(), duplicate])
            .is_err());
        assert_eq!(loaded.export_project_model().unwrap(), before);

        let mut reloaded = SketchManager::new();
        let plan = reloaded.prepare_load_project(before).unwrap();
        commit_plan(&mut reloaded, plan, basis);
        reloaded.scrub_named_views();
        assert_eq!(reloaded.named_views().views[0].id.as_ref(), Some(&id));
        let before = reloaded.export_project_model().unwrap();
        let pending = reloaded.prepare_load_project(before.clone()).unwrap();
        assert!(reloaded.upsert_named_view(view).is_err());
        assert_eq!(reloaded.export_project_model().unwrap(), before);
        commit_plan(&mut reloaded, pending, basis);
        assert_eq!(reloaded.named_views().views[0].id.as_ref(), Some(&id));
    }

    #[test]
    fn named_view_recall_roundtrips_without_moving_geometry() {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let basis = plane.origin_basis().unwrap();
        let mut known_bodies = BTreeSet::new();
        let extrude_body =
            |manager: &mut SketchManager, known: &mut BTreeSet<BodyId>, width: f64| -> BodyId {
                manager.begin_sketch(plane).unwrap();
                manager
                    .add_rectangle_locked(LockedRectangleRequest {
                        mode: crate::dto::RectangleMode::TwoPoint,
                        anchor: crate::Vec2::new(0.0, 0.0),
                        width_mm: Some(width),
                        height_mm: Some(10.0),
                        width_text: Some(width.to_string()),
                        height_text: Some("10".to_string()),
                        corner_hint: crate::Vec2::new(width, 10.0),
                        ctrl_held: false,
                    })
                    .unwrap();
                manager.end_sketch().unwrap();
                let sketch_name = manager.finished_sketches().last().unwrap().name.clone();
                let plan = manager
                    .prepare_extrude(ExtrudeRequest {
                        source_face: None,
                        sketch_name,
                        profile_indices: vec![0],
                        operation: ExtrudeOperation::NewBody,
                        extent: ExtrudeExtent::Distance { distance: 8.0 },
                        taper_angle_deg: 0.0,
                        flip: false,
                        target_body_ids: Vec::new(),
                    })
                    .unwrap();
                commit_plan(manager, plan, basis);
                let body_id = manager
                    .solid_scene()
                    .bodies
                    .iter()
                    .map(|body| body.id)
                    .find(|id| known.insert(*id))
                    .expect("a new body");
                body_id
            };
        let clip = extrude_body(&mut manager, &mut known_bodies, 12.0);
        let housing = extrude_body(&mut manager, &mut known_bodies, 20.0);
        let definitions = manager.extrude_definitions();
        let scene = manager.solid_scene();

        let view = NamedViewConfigurationDto {
            id: None,
            name: "detent".to_string(),
            camera: crate::dto::ViewCameraDto {
                position: [80.0, -40.0, 30.0],
                target: [0.0, 0.0, 8.0],
                up: [0.0, 0.0, 1.0],
            },
            visible_body_ids: vec![clip.0],
            part_offsets: vec![crate::dto::ViewPartOffsetDto {
                body_id: clip.0,
                translation: [0.0, 14.0, 0.0],
            }],
            occurrence_offsets: vec![limo_cad_assembly::ViewOccurrenceOffsetDto {
                occurrence_id: manager.assembly_document().component_structure.occurrences[0].id,
                translation: [3., 0., 2.],
                rotation: [
                    0.,
                    0.,
                    std::f64::consts::FRAC_1_SQRT_2,
                    std::f64::consts::FRAC_1_SQRT_2,
                ],
            }],
            print_layout: true,
            print_bed: Default::default(),
        };
        let unknown = NamedViewConfigurationDto {
            visible_body_ids: vec![999],
            ..view.clone()
        };
        let before = manager.export_project_model().unwrap();
        assert!(manager.set_named_views(vec![unknown]).is_err());
        let collapsed = NamedViewConfigurationDto {
            camera: crate::dto::ViewCameraDto {
                position: [0.0, 0.0, 0.0],
                target: [0.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
            },
            ..view.clone()
        };
        assert!(manager.set_named_views(vec![collapsed]).is_err());
        let distant = NamedViewConfigurationDto {
            camera: crate::dto::ViewCameraDto {
                position: [1.0e20, 0.0, 0.0],
                target: [0.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
            },
            ..view.clone()
        };
        assert!(manager.set_named_views(vec![distant]).is_err());
        let duplicate = NamedViewConfigurationDto {
            visible_body_ids: vec![clip.0, clip.0],
            ..view.clone()
        };
        let deduped = manager.set_named_views(vec![duplicate]).unwrap();
        assert_eq!(deduped.views[0].visible_body_ids, vec![clip.0]);
        assert!(deduped.active.is_none());
        assert_eq!(
            manager.set_named_views(vec![]).unwrap().views.len(),
            0,
            "clear views after the dedup check"
        );
        assert_eq!(manager.export_project_model().unwrap(), before);

        let stored = manager.set_named_views(vec![view]).unwrap();
        assert_eq!(stored.views.len(), 1);
        assert!(stored.active.is_none());
        let names: Vec<_> = manager
            .document()
            .browser()
            .iter()
            .find(|node| node.kind == BrowserNodeKind::NamedViews)
            .unwrap()
            .children
            .iter()
            .map(|node| {
                assert_eq!(node.kind, BrowserNodeKind::NamedView);
                node.name.clone()
            })
            .collect();
        assert_eq!(names, vec![Some("detent".to_string())]);

        let json = manager.export_project_model().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["schema_version"], PROJECT_SCHEMA_VERSION);
        assert_eq!(parsed["views"][0]["name"], "detent");
        assert_eq!(parsed["views"][0]["print_layout"], true);
        assert_eq!(
            parsed["views"][0]["occurrence_offsets"][0]["translation"],
            serde_json::json!([3., 0., 2.])
        );
        assert_eq!(
            parsed["views"][0]["part_offsets"][0]["translation"][1],
            14.0
        );
        assert_eq!(
            parsed["extrudes"],
            serde_json::to_value(&definitions).unwrap()
        );

        let mut legacy = parsed.clone();
        legacy["schema_version"] = serde_json::json!(9);
        legacy.as_object_mut().unwrap().remove("print_intent");
        legacy.as_object_mut().unwrap().remove("views");
        let mut migrated = SketchManager::new();
        let legacy_plan = migrated.prepare_load_project(legacy.to_string()).unwrap();
        commit_plan(&mut migrated, legacy_plan, basis);
        assert!(migrated.named_views().views.is_empty());
        let resaved: serde_json::Value =
            serde_json::from_str(&migrated.export_project_model().unwrap()).unwrap();
        assert_eq!(resaved["schema_version"], PROJECT_SCHEMA_VERSION);
        assert_eq!(resaved["views"], serde_json::json!([]));
        assert_eq!(resaved["extrudes"], parsed["extrudes"]);

        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        commit_plan(&mut loaded, replay, basis);
        assert_eq!(loaded.extrude_definitions(), definitions);
        assert_eq!(loaded.solid_scene().bodies.len(), scene.bodies.len());
        let recalled = loaded.recall_named_view("detent".into()).unwrap();
        assert_eq!(recalled.view.camera.position, [80.0, -40.0, 30.0]);
        assert_eq!(recalled.view.part_offsets[0].translation, [0.0, 14.0, 0.0]);
        assert_eq!(
            recalled.view.occurrence_offsets[0].translation,
            [3., 0., 2.]
        );
        assert!(recalled.view.print_layout);
        assert_eq!(
            recalled.solution,
            loaded.named_view_solution(Some("detent")).unwrap()
        );
        assert_eq!(recalled.visibility.hidden_body_ids, vec![housing.0]);
        let kept = loaded.set_named_views(loaded.named_views.clone()).unwrap();
        assert_eq!(kept.active, None);
        loaded.named_views[0].visible_body_ids.push(999);
        loaded.named_views[0]
            .part_offsets
            .push(crate::dto::ViewPartOffsetDto {
                body_id: 999,
                translation: [1.0, 0.0, 0.0],
            });
        let recalled = loaded.recall_named_view("detent".into()).unwrap();
        assert!(!recalled.view.visible_body_ids.contains(&999));
        assert!(recalled
            .view
            .part_offsets
            .iter()
            .all(|offset| offset.body_id != 999));
        assert_eq!(recalled.visibility.hidden_body_ids, vec![housing.0]);
        assert_eq!(loaded.extrude_definitions(), definitions);
        let scene_after = loaded.solid_scene();
        assert_eq!(scene_after.bodies.len(), scene.bodies.len());
        for (before_body, after_body) in scene.bodies.iter().zip(scene_after.bodies.iter()) {
            assert_eq!(before_body.mesh.positions, after_body.mesh.positions);
        }
        let saved_after: serde_json::Value =
            serde_json::from_str(&loaded.export_project_model().unwrap()).unwrap();
        assert_eq!(saved_after["extrudes"], parsed["extrudes"]);
        assert_eq!(
            saved_after["views"][0]["part_offsets"][0]["translation"],
            serde_json::json!([0.0, 14.0, 0.0])
        );
        assert!(loaded.recall_named_view("missing".into()).is_err());
        assert_eq!(loaded.extrude_definitions(), definitions);
        let visibility = loaded.project_visibility();
        let saved_views = loaded.named_views().views;
        assert_eq!(loaded.clear_named_view().active, None);
        assert_eq!(loaded.named_views().views, saved_views);
        assert_eq!(loaded.project_visibility(), visibility);
        assert_eq!(loaded.extrude_definitions(), definitions);
        let mut all_visible = loaded.project_visibility();
        all_visible.hidden_body_ids.clear();
        loaded.set_project_visibility(all_visible.clone()).unwrap();
        let before_failed_recall = loaded.export_project_model().unwrap();
        let mut incomplete = loaded.assembly_solution();
        incomplete.occurrence_poses.clear();
        *loaded.assembly_solution_cache.borrow_mut() = Some(incomplete);
        assert!(loaded.recall_named_view("detent".into()).is_err());
        assert_eq!(loaded.named_views().active, None);
        assert_eq!(loaded.project_visibility(), all_visible);
        assert_eq!(loaded.export_project_model().unwrap(), before_failed_recall);
        *loaded.assembly_solution_cache.borrow_mut() = None;
        loaded.recall_named_view("detent".into()).unwrap();
        let recompute = loaded.prepare_recompute().unwrap();
        commit_plan(&mut loaded, recompute, basis);
        assert_eq!(loaded.named_views().active, None);
        loaded.recall_named_view("detent".into()).unwrap();
        loaded.begin_sketch(plane).unwrap();
        assert_eq!(loaded.named_views().active, None);
        assert_eq!(loaded.named_views().views, saved_views);
    }

    #[test]
    fn assembly_document_restore_rejects_invalid_snapshots_transactionally() {
        let mut manager = SketchManager::new();
        let before = manager.assembly_document();
        let mut invalid = before.clone();
        invalid.next_joint_id = 0;

        assert!(manager.set_assembly_document(invalid).is_err());
        assert_eq!(manager.assembly_document(), before);
    }

    #[test]
    fn component_occurrences_roundtrip_without_mutating_part_history() {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let basis = plane.origin_basis().unwrap();
        manager.begin_sketch(plane).unwrap();
        manager
            .add_rectangle(RectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                p1: crate::Vec2::new(0.0, 0.0),
                p2: crate::Vec2::new(20.0, 10.0),
                ctrl_held: false,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 15.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let body_id = result_body_ids(&plan.jobs[0])[0];
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(body_id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();

        let history_before = manager.document.features().clone();
        let extrudes_before = manager.solids.definitions().to_vec();
        let promoted = manager
            .assembly_document()
            .component_structure
            .definitions
            .into_iter()
            .find(|definition| definition.body_ids == vec![body_id])
            .unwrap();
        manager
            .update_component(UpdateComponentRequestDto::from(
                limo_cad_assembly::ComponentDefinitionDto {
                    local_coordinate_system: limo_cad_assembly::AssemblyTransformDto {
                        translation: [2.0, 0.0, 0.0],
                        rotation: [0.0, 0.0, 0.0, 1.0],
                    },
                    ..promoted.clone()
                },
            ))
            .unwrap();
        let subassembly = manager
            .create_component(CreateComponentRequestDto {
                name: "Nested fixture".to_string(),
                body_ids: Vec::new(),
                local_coordinate_system: limo_cad_assembly::AssemblyTransformDto::default(),
                absorb_promoted_bodies: false,
            })
            .unwrap();
        let subassembly_occurrence = manager
            .assembly_document()
            .component_structure
            .occurrences
            .into_iter()
            .find(|occurrence| occurrence.component_id == subassembly.id)
            .unwrap();
        manager
            .create_occurrence(CreateOccurrenceRequestDto {
                component_id: promoted.id,
                name: "Nested part".to_string(),
                parent_occurrence_id: Some(subassembly_occurrence.id),
                local_pose: limo_cad_assembly::AssemblyTransformDto {
                    translation: [15.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                },
            })
            .unwrap();
        let duplicate = manager
            .duplicate_occurrence(DuplicateOccurrenceRequestDto {
                occurrence_id: subassembly_occurrence.id,
                parent_occurrence_id: None,
                local_pose: None,
            })
            .unwrap();
        manager
            .set_occurrence_pose(SetOccurrencePoseRequestDto {
                occurrence_id: duplicate.id,
                local_pose: limo_cad_assembly::AssemblyTransformDto {
                    translation: [50.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                },
            })
            .unwrap();

        assert_eq!(manager.document.features(), &history_before);
        assert_eq!(manager.solids.definitions(), extrudes_before.as_slice());
        assert_eq!(manager.solid_scene().bodies.len(), 1);
        assert_eq!(manager.assembly_solution().instance_body_poses.len(), 3);
        let assembly_before = manager.assembly_document();

        let json = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(body_id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();

        assert_eq!(loaded.assembly_document(), assembly_before);
        assert_eq!(loaded.document.features(), &history_before);
        assert_eq!(loaded.solids.definitions(), extrudes_before.as_slice());
        assert_eq!(loaded.solid_scene().bodies.len(), 1);
        assert_eq!(loaded.assembly_solution().instance_body_poses.len(), 3);
    }

    #[test]
    fn project_roundtrip_persists_appearance_and_visibility_and_scrubs_orphans() {
        use limo_cad_core::{BodyAppearance, Rgba8};

        let mut manager = SketchManager::new();
        let basis = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .origin_basis()
        .unwrap();
        let material = limo_cad_core::MaterialDetails {
            kind: "plastic".into(),
            catalog_id: "saved.material".into(),
            warnings: vec!["Saved reference data".into()],
            sources: vec![limo_cad_core::MaterialSource {
                id: "saved.source".into(),
                repository: "test/source".into(),
                revision: "1".repeat(40),
                path: "card.json".into(),
                sha256: "2".repeat(64),
                license: "CC-BY-4.0".into(),
                author: "Test author".into(),
                reference: "https://example.com/card".into(),
            }],
            properties: vec![limo_cad_core::MaterialProperty {
                name: "Density".into(),
                value: limo_cad_core::MaterialValue::Number(1234.56789012345),
                unit: "kg/m^3".into(),
                context: "Engineering reference: saved material".into(),
                source_id: "saved.source".into(),
            }],
            print_profiles: vec![limo_cad_core::MaterialPrintProfile {
                name: "Saved profile".into(),
                source_id: "saved.source".into(),
                compatible_printers: vec!["Test printer".into()],
            }],
        };
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_rectangle_locked(crate::dto::LockedRectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                anchor: crate::Vec2::new(0.0, 0.0),
                width_mm: Some(20.0),
                height_mm: Some(10.0),
                width_text: Some("20".to_string()),
                height_text: Some("10".to_string()),
                corner_hint: crate::Vec2::new(20.0, 10.0),
                ctrl_held: false,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 15.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let body_id = result_body_ids(&plan.jobs[0])[0];
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(body_id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();

        manager
            .set_body_appearance(BodyAppearance {
                body_id,
                color: Rgba8::opaque(200, 40, 40),
                material_name: "PLA Red".to_string(),
                filament_type: "PLA".to_string(),
                brand: "Bambu Lab".to_string(),
                color_name: "Red".to_string(),
                filament_id: Some("GFA00".into()),
                preset_id: Some("bambu.pla.basic.red".into()),
                density_g_cm3: Some(1.24),
                material: Some(material.clone()),
                diameter_mm: 1.75,
            })
            .unwrap();

        manager.body_appearances.push(BodyAppearance {
            body_id: BodyId(999),
            color: Rgba8::opaque(0, 0, 0),
            material_name: "Gone".to_string(),
            filament_type: "PLA".to_string(),
            brand: "Generic".to_string(),
            color_name: String::new(),
            filament_id: None,
            preset_id: None,
            density_g_cm3: None,
            material: None,
            diameter_mm: 1.75,
        });
        let visibility = manager
            .set_project_visibility(ProjectVisibilityDto {
                hidden_body_ids: vec![body_id.0, 999],
                hidden_datum_plane_ids: vec![999],
                hidden_sketch_names: vec!["Sketch1".into(), "DeletedSketch".into()],
            })
            .unwrap();
        assert_eq!(visibility.hidden_body_ids, vec![body_id.0]);
        assert!(visibility.hidden_datum_plane_ids.is_empty());
        assert_eq!(visibility.hidden_sketch_names, vec!["Sketch1"]);

        let json = manager.export_project_model().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let appearances = parsed["body_appearances"].as_array().unwrap();
        assert_eq!(appearances.len(), 1);
        assert_eq!(appearances[0]["body_id"], body_id.0);
        assert_eq!(appearances[0]["material_name"], "PLA Red");
        assert_eq!(appearances[0]["color"]["r"], 200);
        assert_eq!(parsed["visibility"]["hidden_body_ids"][0], body_id.0);
        assert_eq!(parsed["visibility"]["hidden_sketch_names"][0], "Sketch1");
        assert_eq!(
            parsed["visibility"]["hidden_datum_plane_ids"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(body_id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();
        let restored = loaded.body_appearances();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].body_id, body_id);
        assert_eq!(restored[0].material_name, "PLA Red");
        assert_eq!(restored[0].color.r, 200);
        assert_eq!(restored[0].material.as_ref(), Some(&material));
        let before = loaded.export_project_model().unwrap();
        let mut bad = restored[0].clone();
        bad.material.as_mut().unwrap().kind = "invalid".into();
        assert!(loaded.set_body_appearance(bad).is_err());
        assert_eq!(loaded.export_project_model().unwrap(), before);
        let mut bad_project: serde_json::Value = serde_json::from_str(&before).unwrap();
        bad_project["body_appearances"][0]["material"]["kind"] = serde_json::json!("invalid");
        assert!(loaded
            .prepare_load_project(bad_project.to_string())
            .is_err());
        assert_eq!(loaded.export_project_model().unwrap(), before);
        assert_eq!(loaded.project_visibility(), visibility);

        let final_rollback = loaded.document.features().rollback_index;
        let plan = loaded
            .prepare_set_rollback(SetRollbackRequest { rollback_index: 0 })
            .unwrap();
        commit_plan(&mut loaded, plan, basis);
        assert!(loaded.solid_scene().bodies.is_empty());
        assert_eq!(loaded.body_appearances(), restored);
        assert_eq!(loaded.project_visibility(), visibility);

        let staged_json = loaded.export_project_model().unwrap();
        let mut staged = SketchManager::new();
        let plan = staged.prepare_load_project(staged_json).unwrap();
        commit_plan(&mut staged, plan, basis);
        assert!(staged.solid_scene().bodies.is_empty());
        assert_eq!(staged.body_appearances(), restored);
        assert_eq!(staged.project_visibility(), visibility);
        let plan = staged
            .prepare_set_rollback(SetRollbackRequest {
                rollback_index: final_rollback,
            })
            .unwrap();
        commit_plan(&mut staged, plan, basis);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&staged.export_project_model().unwrap())
                .unwrap(),
            parsed,
            "a temporary rollback and staged roundtrip must preserve the complete project"
        );

        let feature_id = staged.extrude_definitions()[0].feature_id;
        let plan = staged
            .prepare_delete_feature(DeleteFeatureRequest { feature_id })
            .unwrap();
        commit_plan(&mut staged, plan, basis);
        assert!(
            staged.body_appearances().is_empty(),
            "actual creator deletion removes its appearance"
        );
        assert!(
            staged.project_visibility().hidden_body_ids.is_empty(),
            "actual creator deletion removes its visibility"
        );
    }

    #[test]
    fn consumed_body_metadata_survives_history_navigation_but_not_creator_deletion() {
        use limo_cad_core::{BodyAppearance, Rgba8};
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let basis = plane.origin_basis().unwrap();
        for name in ["Sketch1", "Sketch2"] {
            manager.begin_sketch(plane).unwrap();
            manager
                .add_rectangle(RectangleRequest {
                    mode: crate::dto::RectangleMode::TwoPoint,
                    p1: crate::Vec2::new(0.0, 0.0),
                    p2: crate::Vec2::new(20.0, 10.0),
                    ctrl_held: false,
                })
                .unwrap();
            manager.end_sketch().unwrap();
            let plan = manager
                .prepare_extrude(ExtrudeRequest {
                    source_face: None,
                    sketch_name: name.to_string(),
                    profile_indices: vec![0],
                    operation: ExtrudeOperation::NewBody,
                    extent: ExtrudeExtent::Distance { distance: 15.0 },
                    taper_angle_deg: 0.0,
                    flip: false,
                    target_body_ids: Vec::new(),
                })
                .unwrap();
            commit_plan(&mut manager, plan, basis);
        }
        let bodies = manager.solid_scene().bodies;
        let target = bodies[0].id;
        let tool = bodies[1].id;
        for body_id in [target, tool] {
            manager
                .set_body_appearance(BodyAppearance {
                    body_id,
                    color: Rgba8::opaque(200, 40, 40),
                    material_name: "PLA Red".into(),
                    filament_type: "PLA".into(),
                    brand: "Generic".into(),
                    color_name: "Red".into(),
                    filament_id: None,
                    preset_id: None,
                    density_g_cm3: None,
                    material: None,
                    diameter_mm: 1.75,
                })
                .unwrap();
        }
        let appearances = manager.body_appearances();
        let visibility = manager
            .set_project_visibility(ProjectVisibilityDto {
                hidden_body_ids: vec![target.0, tool.0],
                ..Default::default()
            })
            .unwrap();
        let before_combine = manager.document.features().rollback_index;
        let plan = manager
            .prepare_body_feature(limo_cad_solid::BodyFeatureRequestDto::Combine(
                limo_cad_solid::CombineRequest {
                    target_body_id: target,
                    tool_body_ids: vec![tool],
                    operation: limo_cad_solid::CombineOperation::Join,
                    keep_tools: false,
                },
            ))
            .unwrap();
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(target, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();
        assert_eq!(
            manager.body_appearances(),
            appearances,
            "consumption retains the tool's historical appearance"
        );
        assert_eq!(manager.project_visibility(), visibility);
        let plan = manager
            .prepare_set_rollback(SetRollbackRequest {
                rollback_index: before_combine,
            })
            .unwrap();
        commit_plan(&mut manager, plan, basis);
        assert_eq!(manager.solid_scene().bodies.len(), 2);
        assert_eq!(manager.body_appearances(), appearances);
        assert_eq!(manager.project_visibility(), visibility);

        let plan = manager
            .prepare_delete_feature(DeleteFeatureRequest {
                feature_id: bodies[0].feature_id,
            })
            .unwrap();
        commit_plan(&mut manager, plan, basis);
        assert_eq!(
            manager
                .body_appearances()
                .iter()
                .map(|a| a.body_id)
                .collect::<Vec<_>>(),
            vec![tool]
        );
        assert_eq!(manager.project_visibility().hidden_body_ids, vec![tool.0]);
    }

    #[test]
    fn project_roundtrip_embeds_and_replays_step_import_source() {
        let mut manager = SketchManager::new();
        let basis = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .origin_basis()
        .unwrap();
        let plan = manager
            .prepare_body_feature(BodyFeatureRequestDto::ImportStep(ImportStepRequest {
                file_name: "fixture.stp".to_string(),
                data_base64: "U1RFUA==".to_string(),
            }))
            .unwrap();
        let KernelJobDto::ImportStep(import) = &plan.jobs[0] else {
            panic!("STEP source should plan an import job");
        };
        let body_id = import.result_body_id;
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(body_id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();

        let json = manager.export_project_model().unwrap();
        assert!(json.contains("\"fixture.stp\""));
        assert!(json.contains("\"U1RFUA==\""));

        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        let KernelJobDto::ImportStep(import) = &replay.jobs[0] else {
            panic!("saved STEP source should replay as an import job");
        };
        assert_eq!(import.result_body_id, body_id);
        assert_eq!(import.data_base64, "U1RFUA==");
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(body_id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();
        assert_eq!(loaded.solid_scene().bodies[0].id, body_id);
    }

    #[test]
    fn timeline_reorder_allows_independent_branches_and_rejects_broken_dependencies() {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let basis = plane.origin_basis().unwrap();
        manager.begin_sketch(plane).unwrap();
        manager
            .add_rectangle(RectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                p1: crate::Vec2::new(0.0, 0.0),
                p2: crate::Vec2::new(20.0, 10.0),
                ctrl_held: false,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 10.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        commit_plan(&mut manager, plan, basis);

        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_circle(CircleRequest {
                mode: crate::dto::CircleMode::CenterDiameter,
                p1: crate::Vec2::new(30.0, 0.0),
                p2: crate::Vec2::new(35.0, 0.0),
                ctrl_held: false,
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let initial = manager.document_dto();
        let sketch1 = initial.features[0].id;
        let extrude = initial.features[1].id;
        let sketch2 = initial.features[2].id;
        let plan = manager
            .prepare_reorder_feature(ReorderFeatureRequest {
                feature_id: sketch2,
                target_index: 0,
            })
            .unwrap();
        commit_plan(&mut manager, plan, basis);
        assert_eq!(
            manager
                .document_dto()
                .features
                .iter()
                .map(|feature| feature.id)
                .collect::<Vec<_>>(),
            vec![sketch2, sketch1, extrude]
        );

        let error = manager
            .prepare_reorder_feature(ReorderFeatureRequest {
                feature_id: extrude,
                target_index: 1,
            })
            .unwrap_err();
        assert!(error.to_string().contains("before its dependency Sketch1"));
        assert_eq!(
            manager
                .document_dto()
                .features
                .iter()
                .map(|feature| feature.id)
                .collect::<Vec<_>>(),
            vec![sketch2, sketch1, extrude]
        );
    }

    #[test]
    fn new_project_replaces_the_current_model_only_after_commit() {
        let mut manager = SketchManager::new();
        manager
            .set_document_name("Existing design".to_string())
            .unwrap();

        let plan = manager.prepare_new_project().unwrap();
        assert_eq!(manager.document().name(), "Existing design");
        assert!(plan.jobs.is_empty());

        let update = manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: Vec::new(),
                    errors: Vec::new(),
                },
            })
            .unwrap();

        assert_eq!(manager.document().name(), "Untitled");
        assert_eq!(update.document.name, "Untitled");
        assert!(update.document.features.is_empty());
        assert!(update.scene.bodies.is_empty());
        assert!(manager.finished_sketches().is_empty());
    }

    #[test]
    fn project_roundtrip_preserves_technical_drawing_intent() {
        let drawing = DrawingDocumentDto {
            sheets: vec![DrawingSheetDto {
                id: 4,
                name: "Assembly overview".to_string(),
                format: DrawingSheetFormat::A3,
                orientation: DrawingSheetOrientation::Landscape,
                standard: DrawingStandard::Iso,
                projection_method: DrawingProjectionMethod::FirstAngle,
                tolerance_note: DrawingToleranceNoteDto {
                    preset: DrawingTolerancePreset::Iso2768Medium,
                    custom: String::new(),
                },
                title_block: DrawingTitleBlockDto {
                    title: "Clamp".to_string(),
                    drawing_number: "NBC-042".to_string(),
                    revision: "B".to_string(),
                    author: "QA".to_string(),
                    ..DrawingTitleBlockDto::default()
                },
                views: vec![DrawingViewDto {
                    scope: Default::default(),
                    occurrence_ids: vec![],
                    id: 9,
                    name: "Front".to_string(),
                    kind: DrawingViewKind::Front,
                    direction: [0.0, -1.0, 0.0],
                    up: [0.0, 0.0, 1.0],
                    position: [120.0, 80.0],
                    scale: 0.5,
                    body_ids: vec![],
                    show_hidden_lines: true,
                    show_tangent_edges: false,
                    parent_view_id: None,
                    alignment: DrawingViewAlignment::Free,
                    derivation: None,
                }],
                annotations: vec![
                    DrawingAnnotationDto::LinearDimension {
                        id: 12,
                        view_id: 9,
                        first: DrawingTopologyAnchorRefDto {
                            topology_signature: None,
                            occurrence_id: None,
                            body_id: BodyId(1),
                            edge_id: limo_cad_core::EdgeId(101),
                            edge_key: "edge:0".to_string(),
                            endpoint: DrawingEdgeEndpoint::Start,
                            fallback_point: [0.0, 0.0, 0.0],
                            circle_center: false,
                        },
                        second: DrawingTopologyAnchorRefDto {
                            topology_signature: None,
                            occurrence_id: None,
                            body_id: BodyId(1),
                            edge_id: limo_cad_core::EdgeId(102),
                            edge_key: "edge:1".to_string(),
                            endpoint: DrawingEdgeEndpoint::End,
                            fallback_point: [20.0, 0.0, 0.0],
                            circle_center: false,
                        },
                        mode: DrawingLinearDimensionMode::Horizontal,
                        offset: -12.0,
                        prefix: String::new(),
                        suffix: " TYP".to_string(),
                        precision: 2,
                        presentation: crate::drawing::DrawingDimensionPresentationDto::default(),
                    },
                    DrawingAnnotationDto::CenterLineBetweenEdges {
                        id: 13,
                        view_id: 9,
                        first: DrawingLineRefDto {
                            topology_signature: None,
                            occurrence_id: None,
                            body_id: BodyId(1),
                            edge_id: limo_cad_core::EdgeId(201),
                            edge_key: "edge:center-left".to_string(),
                            fallback_start: [0.0, 0.0, 0.0],
                            fallback_end: [20.0, 0.0, 0.0],
                        },
                        second: DrawingLineRefDto {
                            topology_signature: None,
                            occurrence_id: None,
                            body_id: BodyId(1),
                            edge_id: limo_cad_core::EdgeId(202),
                            edge_key: "edge:center-right".to_string(),
                            fallback_start: [0.0, 10.0, 0.0],
                            fallback_end: [20.0, 10.0, 0.0],
                        },
                        extension: 2.5,
                    },
                ],
                style: crate::drawing::DrawingSheetStyleDto::default(),
                template_name: String::new(),
                revisions: vec![],
                bom: vec![],
                release: crate::drawing::DrawingReleaseDto::default(),
                revision_table_position: None,
                bom_table_position: None,
            }],
            active_sheet_id: Some(4),
            next_sheet_id: 5,
            next_view_id: 10,
            next_annotation_id: 14,
            next_revision_id: 1,
            next_bom_item_id: 1,
            templates: vec![],
            next_template_id: 1,
        };
        let mut manager = SketchManager::new();
        manager.set_drawing_document(drawing.clone()).unwrap();

        let json = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        assert!(replay.jobs.is_empty());
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto {
                    bodies: Vec::new(),
                    errors: Vec::new(),
                },
            })
            .unwrap();

        assert_eq!(loaded.drawing_document(), drawing);
    }

    #[test]
    fn project_roundtrip_preserves_host_neutral_joint_intent() {
        let connector =
            |body_id, face_id, key: &str, origin| limo_cad_assembly::JointConnectorDto {
                body_id: BodyId(body_id),
                face_id: FaceId(face_id),
                face_key: key.to_string(),
                edge_id: None,
                edge_key: None,
                kind: limo_cad_assembly::JointConnectorKindDto::PlanarFace,
                radius: None,
                source_surface_frame: None,
                frame: limo_cad_assembly::JointFrameDto {
                    origin,
                    primary_axis: [0.0, 0.0, 1.0],
                    secondary_axis: [1.0, 0.0, 0.0],
                },
            };
        let assembly = AssemblyDocumentDto {
            joints: vec![limo_cad_assembly::JointDefinitionDto {
                id: JointId(7),
                name: "Hinge".to_string(),
                kind: limo_cad_assembly::JointKindDto::Revolute,
                connector_a: connector(1, 11, "body-1:face-a", [0.0, 0.0, 0.0]),
                connector_b: connector(2, 22, "body-2:face-b", [0.0, 0.0, 10.0]),
                flipped: true,
                angle_offset_deg: 15.0,
                linear_offset_mm: 0.0,
                limits: Some(limo_cad_assembly::JointLimitsDto {
                    min: -90.0,
                    max: 90.0,
                }),
                angle_limits: None,
                linear_limits: None,
                advanced: limo_cad_assembly::JointAdvancedDto::default(),
                enabled: true,
            }],
            next_joint_id: 8,
            grounded_body_id: Some(BodyId(1)),
            component_structure: limo_cad_assembly::ComponentStructureDto::default(),
            ..AssemblyDocumentDto::default()
        };

        let mut manager = SketchManager::new();
        manager.assembly = assembly.clone();
        let preview = manager
            .preview_joint_motion(SetJointMotionRequestDto {
                joint_id: JointId(7),
                angle_offset_deg: 45.0,
                linear_offset_mm: 0.0,
            })
            .unwrap();
        assert!(preview.solved);
        assert!(preview.body_poses.is_empty());
        assert!(preview.diagnostics.is_empty());

        assert_eq!(manager.assembly_document(), assembly);
        let json = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        assert!(replay.jobs.is_empty());
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto::default(),
            })
            .unwrap();
        assert_eq!(loaded.assembly_document(), assembly);
    }

    pub(super) fn cam_roundtrip_fixture() -> CamDocumentDto {
        CamDocumentDto {
            linking: Vec::new(),
            load_warnings: Vec::new(),
            toolpath_generations: Vec::new(),
            height_expressions: vec![CamOperationHeightExpressionsDto {
                operation_id: 7,
                clearance: CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::StockTop,
                    offset: 8.0,
                },
                retract: CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::StockTop,
                    offset: 2.0,
                },
                feed: CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::StockTop,
                    offset: 1.0,
                },
                top: CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::StockTop,
                    offset: 0.0,
                },
                bottom: Some(CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::StockTop,
                    offset: -1.0,
                }),
            }],
            setups: vec![CamSetupDto {
                id: 3,
                name: "Top setup".to_string(),
                wcs: WorkCoordinateSystemDto::default(),
                wcs_origin: WcsOriginSpecDto::Explicit,
                work_offset: WorkOffset::G55,
                work_offset_count: 1,
                stock_spec: limo_cad_cam::CamStockSpecDto::LegacyBox,
                resolved_stock: limo_cad_cam::CamResolvedStockDto::Box,
                stock: StockBoxDto {
                    min: CamPoint3Dto::new(0.0, 0.0, -12.0),
                    max: CamPoint3Dto::new(30.0, 20.0, 0.0),
                },
                stock_model_box: None,
                body_ids: vec![],
                machine: Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
                    CamPostConfigDto::default(),
                )),
                legacy_clearance_z: None,
                legacy_retract_z: None,
                operations: vec![CamOperationDto::Face {
                    id: 7,
                    name: "Face stock".to_string(),
                    enabled: true,
                    tool_id: 5,
                    bounds: CamRect2Dto {
                        min: CamPoint2Dto::new(0.0, 0.0),
                        max: CamPoint2Dto::new(30.0, 20.0),
                    },
                    top_z: 0.0,
                    target_z: -1.0,
                    step_over: 3.0,
                    step_down: 1.0,
                    safe_distance: 5.0,
                    direction: limo_cad_cam::FaceDirection::BothWays,
                    clearance_z: 8.0,
                    retract_z: 2.0,
                    feed_height_z: 1.0,
                    cutting: CuttingParametersDto {
                        spindle_rpm: 12_000,
                        feed_xy: 800.0,
                        feed_z: 200.0,
                        coolant: CoolantMode::Flood,
                    },
                }],
            }],
            active_setup_id: Some(3),
            tools: vec![CamToolDto {
                id: 5,
                number: Some(1),
                name: "6 mm flat end mill".to_string(),
                kind: CamToolKind::FlatEndMill,
                diameter: 6.0,
                flute_length: 20.0,
                overall_length: 50.0,
                center_cutting: true,
                flute_count: 4,
                point_angle_degrees: None,
                corner_radius: None,
                corner_chamfer: None,
                cutting: CuttingParametersDto::default(),
                cutting_presets: vec![],
                maximum_axial_depth: None,
                default_step_down: None,
                default_step_over: None,
            }],
            units: CamUnits::Millimeters,
            post_defaults: CamPostConfigDto::default(),
            next_setup_id: 4,
            next_operation_id: 8,
            next_tool_id: 6,
        }
    }

    #[test]
    fn project_migration_preserves_cam_chains_and_placed_drawing_intent_together() {
        let mut manager = SketchManager::new();
        let mut cam = cam_roundtrip_fixture();
        cam.height_expressions.clear();
        cam.tools[0].kind = CamToolKind::ChamferMill;
        cam.tools[0].point_angle_degrees = Some(90.0);
        cam.setups[0].operations[0] = serde_json::from_value(serde_json::json!({
            "kind":"chamfer2d", "id":7, "name":"Two chamfer chains", "tool_id":5,
            "path":[{"x":0.0,"y":0.0},{"x":10.0,"y":0.0},{"x":10.0,"y":10.0}],
            "closed":true, "top_z":0.0, "chamfer_width":0.5, "tip_offset":1.0,
            "wall_side":"outside", "clearance_z":8.0, "retract_z":2.0, "feed_height_z":1.0,
            "cutting":{"spindle_rpm":6000,"feed_xy":600.0,"feed_z":100.0,"coolant":"flood"},
            "additional_chains":[{
                "path":[{"x":20.0,"y":0.0},{"x":30.0,"y":0.0},{"x":30.0,"y":10.0}],
                "closed":true,"top_z":-1.0,"chamfer_width":0.5,"wall_side":"outside"
            }]
        }))
        .unwrap();
        manager.set_cam_document(cam.clone()).unwrap();
        manager
            .drawing_command(
                serde_json::from_value(serde_json::json!({
                    "type":"create_sheet", "arguments":{
                        "name":"Manufacturing", "format":"a4", "orientation":"landscape"
                    }
                }))
                .unwrap(),
            )
            .unwrap();
        let mut drawings = manager.drawing_document();
        drawings.sheets[0].views.push(
            serde_json::from_value(serde_json::json!({
                "id":1,"name":"Placed assembly", "kind":"top", "scope":"assembly",
                "occurrence_ids":[], "direction":[0.0,0.0,1.0],"up":[0.0,1.0,0.0],
                "position":[80.0,60.0],"scale":1.0
            }))
            .unwrap(),
        );
        drawings.next_view_id = 2;
        manager.set_drawing_document(drawings.clone()).unwrap();
        let mut model: serde_json::Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        assert_eq!(model["schema_version"], PROJECT_SCHEMA_VERSION);

        for version in [3, 4, 5, 6, 7] {
            model["schema_version"] = version.into();
            if version < 11 {
                model.as_object_mut().unwrap().remove("print_intent");
            }
            let mut loaded = SketchManager::new();
            let plan = loaded.prepare_load_project(model.to_string()).unwrap();
            loaded
                .commit_solid(CommitKernelRequest {
                    transaction_id: plan.transaction_id,
                    scene: KernelSceneDto::default(),
                })
                .unwrap();
            assert_eq!(
                loaded.cam_document(),
                cam,
                "CAM migration from schema {version}"
            );
            assert_eq!(
                loaded.drawing_document(),
                drawings,
                "Drawing migration from schema {version}"
            );
            assert_eq!(
                loaded.cam_document().setups[0].operations[0]
                    .chamfer_chains()
                    .len(),
                2
            );
            let saved: serde_json::Value =
                serde_json::from_str(&loaded.export_project_model().unwrap()).unwrap();
            assert_eq!(saved["schema_version"], PROJECT_SCHEMA_VERSION);
        }
    }

    #[test]
    fn project_roundtrip_preserves_cam_intent_and_regenerates_motion() {
        let cam = cam_roundtrip_fixture();
        let mut manager = SketchManager::new();
        manager.set_cam_document(cam.clone()).unwrap();

        let statuses = manager.cam_toolpath_statuses().unwrap();
        assert_eq!(statuses[0].state, CamToolpathStateDto::NeverGenerated);
        let blocked = manager
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: None,
                program_name: None,
            })
            .unwrap_err();
        assert!(blocked.to_string().contains("NC posting blocked"));
        let regenerated = manager.cam_regenerate_setup(3).unwrap();
        assert_eq!(
            manager.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Current
        );

        let json = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        assert!(replay.jobs.is_empty());
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto {
                    bodies: Vec::new(),
                    errors: Vec::new(),
                },
            })
            .unwrap();

        assert_eq!(loaded.cam_document(), regenerated);
        assert_eq!(
            loaded.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Current
        );
        let program = loaded.cam_plan(3).unwrap();
        assert_eq!(program.stats.operation_count, 1);
        assert!(program.stats.cutting_distance > 0.0);
        let posted = loaded
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: None,
                program_name: None,
            })
            .unwrap();
        assert!(posted.nc.contains("G55"));

        let mut raised_stock = loaded.cam_document();
        raised_stock.setups[0].stock.max.z = 2.0;
        loaded.set_cam_document(raised_stock).unwrap();
        assert_eq!(
            loaded.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Stale
        );
        loaded.cam_regenerate_operation(7).unwrap();
        let CamOperationDto::Face {
            top_z,
            target_z,
            feed_height_z,
            retract_z,
            clearance_z,
            ..
        } = &loaded.cam.setups[0].operations[0]
        else {
            unreachable!();
        };
        assert_eq!(
            (*top_z, *target_z, *feed_height_z, *retract_z, *clearance_z),
            (2.0, 1.0, 3.0, 4.0, 10.0)
        );

        let mut edited = loaded.cam_document();
        edited.tools[0].diameter = 6.1;
        loaded.set_cam_document(edited).unwrap();
        let stale = &loaded.cam_toolpath_statuses().unwrap()[0];
        assert_eq!(stale.state, CamToolpathStateDto::Stale);
        assert!(stale
            .reasons
            .iter()
            .any(|reason| reason.contains("tool definition")));
        assert!(loaded
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: None,
                program_name: None,
            })
            .unwrap_err()
            .to_string()
            .contains("NC posting blocked"));
        loaded.cam_regenerate_operation(7).unwrap();
        assert_eq!(
            loaded.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Current
        );

        let mut setup_edited = loaded.cam_document();
        setup_edited.setups[0].work_offset = WorkOffset::G56;
        loaded.set_cam_document(setup_edited).unwrap();
        let stale = &loaded.cam_toolpath_statuses().unwrap()[0];
        assert_eq!(stale.state, CamToolpathStateDto::Stale);
        assert!(stale
            .reasons
            .iter()
            .any(|reason| reason.contains("Setup, WCS, stock")));
        loaded.cam_regenerate_operation(7).unwrap();

        let mut missing_model = loaded.cam_document();
        missing_model.setups[0].body_ids = vec![BodyId(999)];
        loaded.set_cam_document(missing_model).unwrap();
        let stale = &loaded.cam_toolpath_statuses().unwrap()[0];
        assert_eq!(stale.state, CamToolpathStateDto::Stale);
        assert!(stale
            .reasons
            .iter()
            .any(|reason| reason.contains("CAD model")));
        let missing_error = loaded.cam_regenerate_setup(3).unwrap_err().to_string();
        assert!(missing_error.contains("referenced CAD body 999 no longer exists"));
    }

    #[test]
    fn machine_edits_preserve_generation_but_recheck_production_output() {
        let mut manager = SketchManager::new();
        manager.set_cam_document(cam_roundtrip_fixture()).unwrap();
        manager.cam_regenerate_setup(3).unwrap();
        let before = manager.cam_document();
        let program = manager.cam_plan(3).unwrap();
        let mut generic = before.clone();
        generic.setups[0].machine = None;
        manager.set_cam_document(generic).unwrap();
        assert_eq!(
            manager.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Current
        );
        assert_eq!(manager.cam_plan(3).unwrap(), program);
        assert!(manager
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: None,
                program_name: None
            })
            .unwrap_err()
            .to_string()
            .contains("select a machine/controller"));
        let mut bound = manager.cam_document();
        bound.setups[0].machine = Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
            CamPostConfigDto {
                dialect: limo_cad_cam::PostDialect::Siemens828d,
                siemens_828d: Some(limo_cad_cam::Siemens828dPostConfigDto::default()),
                ..Default::default()
            },
        ));
        let tool_id = bound.tools[0].id;
        bound.setups[0].machine.as_mut().unwrap().tool_calls =
            vec![limo_cad_cam::CamMachineToolBindingDto {
                tool_id,
                call: limo_cad_cam::CamMachineToolCallDto::Name {
                    name: "HostTest_EM6".into(),
                },
            }];
        manager.set_cam_document(bound).unwrap();
        assert_eq!(
            manager.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Current
        );
        assert_eq!(
            manager.cam_document().toolpath_generations,
            before.toolpath_generations
        );
        let output = manager
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: None,
                program_name: None,
            })
            .unwrap();
        assert_eq!(output.dialect, limo_cad_cam::PostDialect::Siemens828d);
        assert!(manager
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: Some(CamPostConfigDto::default()),
                program_name: None
            })
            .unwrap_err()
            .to_string()
            .contains("does not match"));
    }

    #[test]
    fn cam_linking_and_order_invalidate_only_affected_generation_prefixes() {
        let mut cam = cam_roundtrip_fixture();
        let mut second = cam.setups[0].operations[0].clone();
        if let CamOperationDto::Face { id, name, .. } = &mut second {
            *id = 8;
            *name = "Second face".into();
        }
        cam.setups[0].operations.push(second);
        cam.next_operation_id = 9;
        cam.linking.push(limo_cad_cam::CamLinkingDto {
            operation_id: 7,
            ..Default::default()
        });
        let mut manager = SketchManager::new();
        manager.set_cam_document(cam).unwrap();
        manager.cam_regenerate_setup(3).unwrap();

        let mut raised = manager.cam_document();
        raised.setups[0].stock.max.z = 0.2;
        manager.set_cam_document(raised).unwrap();
        manager.cam_regenerate_setup(3).unwrap();
        assert!(manager
            .cam_toolpath_statuses()
            .unwrap()
            .iter()
            .all(|s| s.state == CamToolpathStateDto::Current));
        assert!(manager
            .cam_toolpath_statuses()
            .unwrap()
            .iter()
            .all(|s| s.state == CamToolpathStateDto::Current));
        let original = manager.cam_document();
        let mut edited = original.clone();
        edited.linking[0].lead_in.vertical_radius = 0.4;
        manager.set_cam_document(edited).unwrap();
        assert!(manager
            .cam_toolpath_statuses()
            .unwrap()
            .iter()
            .all(|s| s.state == CamToolpathStateDto::Stale));
        let mut reordered = original.clone();
        reordered.setups[0].operations.swap(0, 1);
        manager.set_cam_document(reordered).unwrap();
        assert!(manager
            .cam_toolpath_statuses()
            .unwrap()
            .iter()
            .all(|s| s.state == CamToolpathStateDto::Stale));
        assert!(manager
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: None,
                program_name: None
            })
            .unwrap_err()
            .to_string()
            .contains("NC posting blocked"));
        manager.cam_regenerate_setup(3).unwrap();
        let mut edited = manager.cam_document();
        edited.linking[0].lead_in_feed *= 0.5;
        manager.set_cam_document(edited).unwrap();
        let statuses = manager.cam_toolpath_statuses().unwrap();
        assert_eq!(
            statuses.iter().find(|s| s.operation_id == 8).unwrap().state,
            CamToolpathStateDto::Current
        );
        assert_eq!(
            statuses.iter().find(|s| s.operation_id == 7).unwrap().state,
            CamToolpathStateDto::Stale
        );
        let mut ordered = manager.cam_document();
        let mut rest = ordered.setups[0].clone();
        rest.id = 4;
        rest.name = "Rest setup".into();
        rest.operations.clear();
        rest.resolved_stock = CamResolvedStockDto::Rest { source_setup_id: 3 };
        rest.stock_spec = limo_cad_cam::CamStockSpecDto::RestFromSetup { setup_id: 3 };
        ordered.setups.push(rest);
        ordered.next_setup_id = 5;
        manager.set_cam_document(ordered.clone()).unwrap();
        ordered.setups.swap(0, 1);
        let error = manager.set_cam_document(ordered).unwrap_err().to_string();
        assert!(error.contains("must follow"), "{error}");
        let json = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto::default(),
            })
            .unwrap();
        assert_eq!(
            loaded.cam_document().linking,
            manager.cam_document().linking
        );
        assert_eq!(loaded.cam_document().setups[0].operations[0].id(), 8);
    }

    #[test]
    fn contour_regeneration_re_resolves_current_model_edges_and_fails_on_broken_refs() {
        let mut manager = SketchManager::new();
        let basis = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .origin_basis()
        .unwrap();
        let plan = manager
            .prepare_body_feature(BodyFeatureRequestDto::ImportStep(ImportStepRequest {
                file_name: "associative-contour.stp".into(),
                data_base64: "U1RFUA==".into(),
            }))
            .unwrap();
        let body_id = result_body_ids(&plan.jobs[0])[0];
        let mut body = raw_body(body_id, basis);
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body.clone()],
                    errors: vec![],
                },
            })
            .unwrap();

        let cam = CamDocumentDto {
            tools: vec![CamToolDto {
                id: 1,
                number: Some(1),
                name: "EM4".into(),
                kind: CamToolKind::FlatEndMill,
                diameter: 4.0,
                flute_length: 12.0,
                overall_length: 35.0,
                center_cutting: true,
                flute_count: 3,
                point_angle_degrees: None,
                corner_radius: None,
                corner_chamfer: None,
                cutting: CuttingParametersDto::default(),
                cutting_presets: vec![],
                maximum_axial_depth: None,
                default_step_down: None,
                default_step_over: None,
            }],
            setups: vec![CamSetupDto {
                id: 1,
                name: "Associative setup".into(),
                wcs: WorkCoordinateSystemDto::default(),
                wcs_origin: WcsOriginSpecDto::Explicit,
                work_offset: WorkOffset::G54,
                work_offset_count: 1,
                stock_spec: limo_cad_cam::CamStockSpecDto::LegacyBox,
                resolved_stock: CamResolvedStockDto::Box,
                stock: StockBoxDto {
                    min: CamPoint3Dto::new(-5.0, -5.0, -5.0),
                    max: CamPoint3Dto::new(30.0, 15.0, 0.0),
                },
                stock_model_box: None,
                body_ids: vec![body_id],
                machine: None,
                legacy_clearance_z: None,
                legacy_retract_z: None,
                operations: vec![CamOperationDto::Contour2d {
                    id: 1,
                    name: "Associative edge".into(),
                    enabled: true,
                    tool_id: 1,
                    path: vec![CamPoint2Dto::new(5.0, 5.0), CamPoint2Dto::new(6.0, 5.0)],
                    closed: false,
                    top_z: 0.0,
                    bottom_z: -1.0,
                    step_down: 1.0,
                    compensation: ContourCompensation::On,
                    compensation_mode: CompensationMode::InSoftware,
                    lead_in: 2.0,
                    lead_out: 2.0,
                    lead_arc_radius: None,
                    direction: MillingDirection::Climb,
                    roughing_passes: 1,
                    roughing_step_over: None,
                    finishing_pass: false,
                    finish_allowance: 0.0,
                    finish_feed: None,
                    spring_pass: false,
                    chain_ref: Some(CamChainRefDto {
                        source: CamChainSource::Model,
                        keys: vec![format!("edge:{}:edge:0", body_id.0)],
                        reversed: false,
                    }),
                    clearance_z: 8.0,
                    retract_z: 3.0,
                    feed_height_z: 1.0,
                    cutting: CuttingParametersDto {
                        spindle_rpm: 8_000,
                        feed_xy: 500.0,
                        feed_z: 150.0,
                        coolant: CoolantMode::Flood,
                    },
                }],
            }],
            active_setup_id: Some(1),
            next_setup_id: 2,
            next_operation_id: 2,
            next_tool_id: 2,
            ..Default::default()
        };
        manager.set_cam_document(cam).unwrap();

        manager.cam_regenerate_operation(1).unwrap();
        let resolved_path = |manager: &SketchManager| match &manager.cam.setups[0].operations[0] {
            CamOperationDto::Contour2d { path, .. } => path.clone(),
            _ => unreachable!(),
        };
        assert_eq!(
            resolved_path(&manager),
            vec![CamPoint2Dto::new(0.0, 0.0), CamPoint2Dto::new(20.0, 0.0)]
        );

        body.edges[0].points[1].x = 24.0;
        let recompute = manager.prepare_recompute().unwrap();
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: recompute.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body],
                    errors: vec![],
                },
            })
            .unwrap();
        manager.cam_regenerate_operation(1).unwrap();
        assert_eq!(resolved_path(&manager)[1], CamPoint2Dto::new(24.0, 0.0));

        let mut broken = manager.cam_document();
        let CamOperationDto::Contour2d {
            chain_ref: Some(reference),
            ..
        } = &mut broken.setups[0].operations[0]
        else {
            unreachable!();
        };
        reference.keys[0] = format!("edge:{}:missing", body_id.0);
        manager.set_cam_document(broken).unwrap();
        let before = manager.cam_document();
        let error = manager.cam_regenerate_operation(1).unwrap_err().to_string();
        assert!(error.contains("no longer exists"));
        assert!(error.contains("Reselect"));
        assert_eq!(
            manager.cam_document(),
            before,
            "failed resolution is transactional"
        );
    }

    #[test]
    fn hole_regeneration_re_resolves_current_cylindrical_face() {
        let mut manager = SketchManager::new();
        let basis = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .origin_basis()
        .unwrap();
        let plan = manager
            .prepare_body_feature(BodyFeatureRequestDto::ImportStep(ImportStepRequest {
                file_name: "associative-hole.stp".into(),
                data_base64: "U1RFUA==".into(),
            }))
            .unwrap();
        let body_id = result_body_ids(&plan.jobs[0])[0];
        let mut body = raw_body(body_id, basis);
        body.positions = vec![4.0, 3.0, 0.0, 6.0, 3.0, 0.0, 4.0, 3.0, -8.0, 6.0, 3.0, -8.0];
        body.normals = [0.0_f32, -1.0, 0.0].repeat(4);
        body.indices = vec![0, 1, 2, 1, 3, 2];
        body.faces[0].first_index = 0;
        body.faces[0].index_count = 6;
        body.faces[0].plane = None;
        body.faces[0].signature = None;
        body.faces[0].cylinder = Some(CylindricalSurfaceDto {
            origin: Point3Dto::from([5.0, 3.0, 0.0]),
            axis: Point3Dto::from([0.0, 0.0, 1.0]),
            reference: Point3Dto::from([1.0, 0.0, 0.0]),
            radius: 1.0,
        });
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body.clone()],
                    errors: vec![],
                },
            })
            .unwrap();
        let face_id = manager.solids.scene().bodies[0].faces[0].id.0;
        let face_key = format!("{}:{face_id}", body_id.0);

        let cam = CamDocumentDto {
            tools: vec![CamToolDto {
                id: 1,
                number: Some(1),
                name: "D1".into(),
                kind: CamToolKind::Drill,
                diameter: 1.0,
                flute_length: 20.0,
                overall_length: 40.0,
                center_cutting: true,
                flute_count: 2,
                point_angle_degrees: Some(118.0),
                corner_radius: None,
                corner_chamfer: None,
                cutting: CuttingParametersDto::default(),
                cutting_presets: vec![],
                maximum_axial_depth: None,
                default_step_down: None,
                default_step_over: None,
            }],
            setups: vec![CamSetupDto {
                id: 1,
                name: "Hole setup".into(),
                wcs: WorkCoordinateSystemDto::default(),
                wcs_origin: WcsOriginSpecDto::Explicit,
                work_offset: WorkOffset::G54,
                work_offset_count: 1,
                stock_spec: limo_cad_cam::CamStockSpecDto::LegacyBox,
                resolved_stock: CamResolvedStockDto::Box,
                stock: StockBoxDto {
                    min: CamPoint3Dto::new(0.0, 0.0, -10.0),
                    max: CamPoint3Dto::new(15.0, 10.0, 0.0),
                },
                stock_model_box: None,
                body_ids: vec![body_id],
                machine: None,
                legacy_clearance_z: None,
                legacy_retract_z: None,
                operations: vec![CamOperationDto::Drill {
                    id: 1,
                    name: "Associative hole".into(),
                    enabled: true,
                    tool_id: 1,
                    points: vec![],
                    holes: vec![CamHoleDto {
                        point: CamPoint2Dto::new(9.0, 9.0),
                        top_z: -1.0,
                        bottom_z: -2.0,
                        axis: [0.0, 0.0, 1.0],
                        face_key: Some(face_key),
                    }],
                    top_z: 0.0,
                    bottom_z: -8.0,
                    retract_z: 2.0,
                    drill_tip_through: false,
                    breakthrough_depth: 0.0,
                    peck_depth: None,
                    dwell_seconds: 0.0,
                    clearance_z: 5.0,
                    feed_height_z: 1.0,
                    cycle: DrillCycle::Drill,
                    peck_retract: None,
                    thread_pitch: None,
                    floating_tap_holder: false,
                    feed_out: None,
                    cutting: CuttingParametersDto {
                        spindle_rpm: 4_000,
                        feed_xy: 200.0,
                        feed_z: 100.0,
                        coolant: CoolantMode::Flood,
                    },
                }],
            }],
            active_setup_id: Some(1),
            next_setup_id: 2,
            next_operation_id: 2,
            next_tool_id: 2,
            ..Default::default()
        };
        manager.set_cam_document(cam).unwrap();
        manager.cam_regenerate_operation(1).unwrap();
        let resolved_hole = |manager: &SketchManager| match &manager.cam.setups[0].operations[0] {
            CamOperationDto::Drill { holes, .. } => holes[0].clone(),
            _ => unreachable!(),
        };
        let hole = resolved_hole(&manager);
        assert_eq!(hole.point, CamPoint2Dto::new(5.0, 3.0));
        assert!((hole.top_z - 0.0).abs() < 1.0e-9);
        assert!((hole.bottom_z - -8.0).abs() < 1.0e-9);

        // An operation Bottom that does not reference the holes is the depth
        // for every picked hole, past the end of the picked face; the
        // hole-bottom reference keeps each face's own span.
        let expression = |reference, offset| limo_cad_cam::CamHeightExpressionDto {
            reference,
            geometry: None,
            offset,
        };
        let heights = |bottom| CamOperationHeightExpressionsDto {
            operation_id: 1,
            clearance: expression(CamHeightReferenceDto::Origin, 5.0),
            retract: expression(CamHeightReferenceDto::Origin, 2.0),
            feed: expression(CamHeightReferenceDto::Origin, 1.0),
            top: expression(CamHeightReferenceDto::HoleTop, 0.0),
            bottom: Some(bottom),
        };
        manager.cam.height_expressions =
            vec![heights(expression(CamHeightReferenceDto::Origin, -9.5))];
        manager.cam_regenerate_operation(1).unwrap();
        assert!((resolved_hole(&manager).bottom_z - -9.5).abs() < 1.0e-9);
        assert!((resolved_hole(&manager).top_z - 0.0).abs() < 1.0e-9);
        manager.cam.height_expressions =
            vec![heights(expression(CamHeightReferenceDto::HoleBottom, 0.0))];
        manager.cam_regenerate_operation(1).unwrap();
        assert!((resolved_hole(&manager).bottom_z - -8.0).abs() < 1.0e-9);
        manager.cam.height_expressions.clear();

        for point in body.positions.as_chunks_mut::<3>().0 {
            point[0] += 2.0;
        }
        body.faces[0].cylinder.as_mut().unwrap().origin.x += 2.0;
        let recompute = manager.prepare_recompute().unwrap();
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: recompute.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body.clone()],
                    errors: vec![],
                },
            })
            .unwrap();
        manager.cam_regenerate_operation(1).unwrap();
        assert_eq!(resolved_hole(&manager).point, CamPoint2Dto::new(7.0, 3.0));

        body.faces[0].cylinder = None;
        let recompute = manager.prepare_recompute().unwrap();
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: recompute.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body],
                    errors: vec![],
                },
            })
            .unwrap();
        let before = manager.cam_document();
        let error = manager.cam_regenerate_operation(1).unwrap_err().to_string();
        assert!(error.contains("no longer cylindrical"));
        assert_eq!(
            manager.cam_document(),
            before,
            "failed resolution is transactional"
        );
    }

    #[test]
    fn adaptive_regeneration_captures_current_cad_and_preserves_it_on_failure() {
        let mut manager = SketchManager::new();
        let basis = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .origin_basis()
        .unwrap();
        let plan = manager
            .prepare_body_feature(BodyFeatureRequestDto::ImportStep(ImportStepRequest {
                file_name: "adaptive-fixture.stp".into(),
                data_base64: "U1RFUA==".into(),
            }))
            .unwrap();
        let body_id = result_body_ids(&plan.jobs[0])[0];
        let mut body = raw_body(body_id, basis);
        body.positions = vec![4.0, 3.0, 0.0, 6.0, 3.0, 0.0, 6.0, 5.0, 0.0, 4.0, 5.0, 0.0];
        body.normals = vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        body.indices = vec![0, 1, 2, 0, 2, 3];
        body.faces[0].index_count = 6;
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body.clone()],
                    errors: vec![],
                },
            })
            .unwrap();
        let cam = CamDocumentDto {
            tools: vec![CamToolDto {
                id: 1,
                number: Some(1),
                name: "EM4".into(),
                kind: CamToolKind::FlatEndMill,
                diameter: 4.0,
                flute_length: 12.0,
                overall_length: 35.0,
                center_cutting: true,
                flute_count: 3,
                point_angle_degrees: None,
                corner_radius: None,
                corner_chamfer: None,
                cutting: CuttingParametersDto::default(),
                cutting_presets: vec![],
                maximum_axial_depth: None,
                default_step_down: None,
                default_step_over: None,
            }],
            setups: vec![CamSetupDto {
                id: 1,
                name: "Adaptive setup".into(),
                wcs: WorkCoordinateSystemDto::default(),
                wcs_origin: WcsOriginSpecDto::Explicit,
                work_offset: WorkOffset::G54,
                work_offset_count: 1,
                stock_spec: limo_cad_cam::CamStockSpecDto::LegacyBox,
                resolved_stock: CamResolvedStockDto::Box,
                stock: StockBoxDto {
                    min: CamPoint3Dto::new(0.0, 0.0, -2.0),
                    max: CamPoint3Dto::new(10.0, 8.0, 0.0),
                },
                stock_model_box: None,
                body_ids: vec![body_id],
                machine: None,
                legacy_clearance_z: None,
                legacy_retract_z: None,
                operations: vec![CamOperationDto::Adaptive3d {
                    id: 1,
                    name: "Adaptive".into(),
                    enabled: true,
                    tool_id: 1,
                    top_z: 0.0,
                    bottom_z: -1.0,
                    clearance_z: 5.0,
                    retract_z: 3.0,
                    feed_height_z: 1.0,
                    cutting: CuttingParametersDto::default(),
                    geometry: None,
                    parameters: limo_cad_cam::CamAdaptiveParametersDto {
                        optimal_load: 1.0,
                        maximum_stepdown: 1.0,
                        minimum_cutting_radius: 0.8,
                        radial_stock_to_leave: 0.1,
                        axial_stock_to_leave: 0.1,
                        tolerance: 0.3,
                        ramp_angle_degrees: 3.0,
                        maximum_ramp_stepdown: 0.5,
                        ramp_feed: 100.0,
                        linking_feed: 600.0,
                        stay_down_distance: 8.0,
                        machine_cavities: true,
                    },
                }],
            }],
            active_setup_id: Some(1),
            next_setup_id: 2,
            next_tool_id: 2,
            next_operation_id: 2,
            height_expressions: vec![CamOperationHeightExpressionsDto {
                operation_id: 1,
                top: CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::ModelTop,
                    offset: -0.25,
                },
                bottom: Some(CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::Origin,
                    offset: -1.0,
                }),
                feed: CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::StockTop,
                    offset: 1.0,
                },
                retract: CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::StockTop,
                    offset: 3.0,
                },
                clearance: CamHeightExpressionDto {
                    geometry: None,
                    reference: CamHeightReferenceDto::StockTop,
                    offset: 5.0,
                },
            }],
            ..Default::default()
        };
        manager.set_cam_document(cam).unwrap();
        manager.cam_regenerate_operation(1).unwrap();
        assert_eq!(
            manager.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Current
        );
        let snapshot = |m: &SketchManager| match &m.cam.setups[0].operations[0] {
            CamOperationDto::Adaptive3d {
                geometry: Some(g), ..
            } => g.targets[0].clone(),
            _ => panic!("snapshot missing"),
        };
        let resolved_top = |m: &SketchManager| match m.cam.setups[0].operations[0] {
            CamOperationDto::Adaptive3d { top_z, .. } => top_z,
            _ => unreachable!(),
        };
        assert_eq!(resolved_top(&manager), -0.25);
        assert_eq!(
            snapshot(&manager).positions,
            body.positions
                .iter()
                .map(|&v| f64::from(v))
                .collect::<Vec<_>>()
        );
        let project = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(project).unwrap();
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body.clone()],
                    errors: vec![],
                },
            })
            .unwrap();
        assert_eq!(snapshot(&manager), snapshot(&loaded));
        assert_eq!(resolved_top(&loaded), -0.25);
        assert_eq!(
            loaded.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Current
        );

        for point in body.positions.as_chunks_mut::<3>().0 {
            point[0] += 1.0;
            point[2] += 0.5;
        }
        let plan = loaded.prepare_recompute().unwrap();
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body.clone()],
                    errors: vec![],
                },
            })
            .unwrap();
        assert_eq!(
            loaded.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Stale
        );
        assert_ne!(snapshot(&loaded).positions[0], f64::from(body.positions[0]));
        loaded.cam_regenerate_setup(1).unwrap();
        assert_eq!(snapshot(&loaded).positions[0], f64::from(body.positions[0]));
        assert_eq!(
            resolved_top(&loaded),
            0.25,
            "Top must follow the current CAD height, including an air offset above stock"
        );
        assert_eq!(
            loaded.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Current
        );

        let prior = loaded.cam_document();

        body.positions = vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 8.0, 0.0, 0.0, 8.0, 0.0];
        let plan = loaded.prepare_recompute().unwrap();
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![body],
                    errors: vec![],
                },
            })
            .unwrap();
        assert!(loaded
            .cam_regenerate_operation(1)
            .unwrap_err()
            .to_string()
            .contains("no accessible"));
        assert_eq!(loaded.cam_document(), prior);
        assert_eq!(
            loaded.cam_toolpath_statuses().unwrap()[0].state,
            CamToolpathStateDto::Stale
        );
    }

    /// Recursively remove every object entry named `key`, simulating a project
    /// written before that field existed.
    fn strip_json_key(value: &mut serde_json::Value, key: &str) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove(key);
                for child in map.values_mut() {
                    strip_json_key(child, key);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    strip_json_key(child, key);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn legacy_cam_document_without_feed_height_opens_clean() {
        let cam = CamDocumentDto {
            load_warnings: Vec::new(),
            toolpath_generations: Vec::new(),
            height_expressions: Vec::new(),
            linking: Vec::new(),
            setups: vec![CamSetupDto {
                id: 3,
                name: "Top setup".to_string(),
                wcs: WorkCoordinateSystemDto::default(),
                wcs_origin: WcsOriginSpecDto::Explicit,
                work_offset: WorkOffset::G54,
                work_offset_count: 1,
                stock_spec: limo_cad_cam::CamStockSpecDto::LegacyBox,
                resolved_stock: limo_cad_cam::CamResolvedStockDto::Box,
                stock: StockBoxDto {
                    min: CamPoint3Dto::new(0.0, 0.0, 0.0),
                    max: CamPoint3Dto::new(30.0, 20.0, 14.0),
                },
                stock_model_box: None,
                body_ids: vec![],
                machine: None,
                legacy_clearance_z: None,
                legacy_retract_z: None,
                operations: vec![CamOperationDto::Face {
                    id: 7,
                    name: "Face stock".to_string(),
                    enabled: true,
                    tool_id: 5,
                    bounds: CamRect2Dto {
                        min: CamPoint2Dto::new(0.0, 0.0),
                        max: CamPoint2Dto::new(30.0, 20.0),
                    },
                    top_z: 14.0,
                    target_z: 13.0,
                    step_over: 3.0,
                    step_down: 1.0,
                    safe_distance: 5.0,
                    direction: limo_cad_cam::FaceDirection::BothWays,
                    clearance_z: 20.0,
                    retract_z: 17.0,
                    feed_height_z: 15.0,
                    cutting: CuttingParametersDto {
                        spindle_rpm: 12_000,
                        feed_xy: 800.0,
                        feed_z: 200.0,
                        coolant: CoolantMode::Flood,
                    },
                }],
            }],
            active_setup_id: Some(3),
            tools: vec![CamToolDto {
                id: 5,
                number: Some(1),
                name: "6 mm flat end mill".to_string(),
                kind: CamToolKind::FlatEndMill,
                diameter: 6.0,
                flute_length: 20.0,
                overall_length: 50.0,
                center_cutting: true,
                flute_count: 4,
                point_angle_degrees: None,
                corner_radius: None,
                corner_chamfer: None,
                cutting: CuttingParametersDto::default(),
                cutting_presets: vec![],
                maximum_axial_depth: None,
                default_step_down: None,
                default_step_over: None,
            }],
            units: CamUnits::Millimeters,
            post_defaults: CamPostConfigDto::default(),
            next_setup_id: 4,
            next_operation_id: 8,
            next_tool_id: 6,
        };
        let mut manager = SketchManager::new();
        manager.set_cam_document(cam).unwrap();

        let json = manager.export_project_model().unwrap();
        let mut legacy: serde_json::Value = serde_json::from_str(&json).unwrap();
        strip_json_key(&mut legacy, "feed_height_z");
        let legacy_json = serde_json::to_string(&legacy).unwrap();

        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(legacy_json).unwrap();
        assert!(replay.jobs.is_empty());
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto::default(),
            })
            .unwrap();

        let reopened = loaded.cam_document();
        assert!(reopened.load_warnings.is_empty());
        let CamOperationDto::Face {
            enabled,
            feed_height_z,
            ..
        } = &reopened.setups[0].operations[0]
        else {
            panic!("expected the face operation to survive the legacy load");
        };
        assert!(enabled);

        assert_eq!(*feed_height_z, 14.0);
        let program = loaded.cam_plan(3).unwrap();
        assert_eq!(program.stats.operation_count, 1);
    }

    #[test]
    fn committed_feature_deletion_removes_joints_that_reference_its_body() {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let basis = plane.origin_basis().unwrap();

        for expected_name in ["Sketch1", "Sketch2"] {
            manager.begin_sketch(plane).unwrap();
            manager
                .add_rectangle(RectangleRequest {
                    mode: crate::dto::RectangleMode::TwoPoint,
                    p1: crate::Vec2::new(0.0, 0.0),
                    p2: crate::Vec2::new(20.0, 10.0),
                    ctrl_held: false,
                })
                .unwrap();
            manager.end_sketch().unwrap();
            let plan = manager
                .prepare_extrude(ExtrudeRequest {
                    source_face: None,
                    sketch_name: expected_name.to_string(),
                    profile_indices: vec![0],
                    operation: ExtrudeOperation::NewBody,
                    extent: ExtrudeExtent::Distance { distance: 15.0 },
                    taper_angle_deg: 0.0,
                    flip: false,
                    target_body_ids: Vec::new(),
                })
                .unwrap();
            commit_plan(&mut manager, plan, basis);
        }

        let scene = manager.solid_scene();
        assert_eq!(scene.bodies.len(), 2);
        let connector = |body: &limo_cad_solid::BodyDto| {
            let face = &body.faces[0];
            let face_basis = face.plane.unwrap();
            limo_cad_assembly::JointConnectorDto {
                body_id: body.id,
                face_id: face.id,
                face_key: face.key.clone(),
                edge_id: None,
                edge_key: None,
                kind: limo_cad_assembly::JointConnectorKindDto::PlanarFace,
                radius: None,
                source_surface_frame: None,
                frame: limo_cad_assembly::JointFrameDto {
                    origin: face_basis.origin,
                    primary_axis: face_basis.normal,
                    secondary_axis: face_basis.u,
                },
            }
        };
        manager
            .create_joint(CreateJointRequestDto {
                name: "Disposable mate".to_string(),
                kind: limo_cad_assembly::JointKindDto::Rigid,
                connector_a: connector(&scene.bodies[0]),
                connector_b: connector(&scene.bodies[1]),
                flipped: true,
                angle_offset_deg: 0.0,
                linear_offset_mm: 0.0,
                limits: None,
                angle_limits: None,
                linear_limits: None,
                advanced: limo_cad_assembly::JointAdvancedDto::default(),
                grounded_body_id: Some(scene.bodies[1].id),
                grounded_occurrence_id: None,
            })
            .unwrap();
        assert_eq!(manager.assembly_document().joints.len(), 1);

        let deleted_body = &scene.bodies[0];
        let retained_body = &scene.bodies[1];
        let plan = manager
            .prepare_delete_feature(DeleteFeatureRequest {
                feature_id: deleted_body.feature_id,
            })
            .unwrap();
        assert_eq!(
            manager.assembly_document().joints.len(),
            1,
            "joint intent must remain intact until the kernel transaction commits"
        );
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![raw_body(retained_body.id, basis)],
                    errors: Vec::new(),
                },
            })
            .unwrap();
        assert!(manager.assembly_document().joints.is_empty());
    }

    #[test]
    fn nested_sketch_loops_plan_one_material_region_with_an_inner_wire() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        for (p1, p2) in [
            (crate::Vec2::new(0.0, 0.0), crate::Vec2::new(40.0, 40.0)),
            (crate::Vec2::new(10.0, 10.0), crate::Vec2::new(30.0, 30.0)),
        ] {
            manager
                .add_rectangle(RectangleRequest {
                    mode: crate::dto::RectangleMode::TwoPoint,
                    p1,
                    p2,
                    ctrl_held: false,
                })
                .unwrap();
        }
        manager.end_sketch().unwrap();

        let catalog = manager.profile_catalog();
        let profiles = &catalog[0].profiles;
        assert_eq!(profiles.len(), 2);
        let outer = profiles
            .iter()
            .max_by(|a, b| a.area.total_cmp(&b.area))
            .unwrap();
        let inner = profiles
            .iter()
            .min_by(|a, b| a.area.total_cmp(&b.area))
            .unwrap();
        assert_eq!(outer.parent_index, None);
        assert_eq!(outer.nesting_depth, 0);
        assert_eq!(inner.parent_index, Some(outer.index));
        assert_eq!(inner.nesting_depth, 1);

        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![inner.index],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 10.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("nested profile should plan an Extrude job");
        };
        assert_eq!(job.profiles.len(), 1);
        assert_eq!(job.profiles[0].profile_index, outer.index);
        assert_eq!(job.profiles[0].holes.len(), 1);
        assert_eq!(job.profiles[0].holes[0].profile_index, inner.index);
        assert_eq!(job.result_body_ids.len(), 1);
    }

    /// Live desktop regression from 2026-08-12: two R10 fillets consume the
    /// entire top edge of a 20 mm-wide rectangle. A centerline runs from the
    /// now-shared arc endpoint into a concentric circle. Profile discovery
    /// must ignore that graph bridge and expose one outer material region
    /// with one hole to Extrude.
    #[test]
    fn fully_consumed_edge_arch_with_centerline_and_circle_is_extrudable() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_rectangle(RectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                p1: crate::Vec2::new(-10.0, 0.0),
                p2: crate::Vec2::new(10.0, 40.0),
                ctrl_held: true,
            })
            .unwrap();
        let lines = manager.active_snapshot().unwrap();
        let line = |horizontal: bool, ordinate: f64| {
            lines
                .entities
                .iter()
                .find_map(|entity| match entity {
                    crate::dto::EntityDto::Line { id, start, end, .. }
                        if if horizontal {
                            (start.y - ordinate).abs() < 1e-8 && (end.y - ordinate).abs() < 1e-8
                        } else {
                            (start.x - ordinate).abs() < 1e-8 && (end.x - ordinate).abs() < 1e-8
                        } =>
                    {
                        Some(*id)
                    }
                    _ => None,
                })
                .unwrap()
        };
        let top = line(true, 40.0);
        let right = line(false, 10.0);
        let left = line(false, -10.0);
        manager
            .fillet_lines(FilletRequest {
                l1: top,
                l2: right,
                radius_text: "10".to_string(),
            })
            .unwrap();
        manager
            .fillet_lines(FilletRequest {
                l1: top,
                l2: left,
                radius_text: "10".to_string(),
            })
            .unwrap();
        manager
            .add_line(SegmentRequest {
                from: crate::Vec2::new(0.0, 40.0),
                to_raw: crate::Vec2::new(0.0, 30.0),
                ctrl_held: true,
            })
            .unwrap();
        manager
            .add_circle_locked(LockedCircleRequest {
                mode: crate::dto::CircleMode::CenterDiameter,
                anchor: crate::Vec2::new(0.0, 30.0),
                diameter_mm: Some(10.0),
                diameter_text: None,
                edge_hint: crate::Vec2::new(5.0, 30.0),
                ctrl_held: true,
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let catalog = manager.profile_catalog();
        assert_eq!(catalog[0].profiles.len(), 2, "outer boundary plus hole");
        let outer = catalog[0]
            .profiles
            .iter()
            .find(|profile| profile.nesting_depth == 0)
            .unwrap();
        let hole = catalog[0]
            .profiles
            .iter()
            .find(|profile| profile.nesting_depth == 1)
            .unwrap();
        assert_eq!(hole.parent_index, Some(outer.index));
        assert_eq!(
            outer.curves.len(),
            4,
            "three straight edges and one canonical semicircle form the outer wire"
        );
        assert!(matches!(
            outer.curves.iter().find(|curve| matches!(curve, ProfileCurveDto::Arc { .. })),
            Some(ProfileCurveDto::Arc { source_entity_ids, .. }) if source_entity_ids.len() == 2
        ));

        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![outer.index],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 10.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("arch profile should plan an Extrude job");
        };
        assert_eq!(job.profiles.len(), 1);
        assert_eq!(job.profiles[0].holes.len(), 1);
        assert_eq!(job.profiles[0].curves.len(), 4);
        assert_eq!(job.profiles[0].holes[0].curves.len(), 1);
    }

    #[test]
    fn circle_divided_by_a_crossing_line_uses_partial_analytic_arcs() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_circle_locked(LockedCircleRequest {
                mode: crate::dto::CircleMode::CenterDiameter,
                anchor: crate::Vec2::new(0.0, 0.0),
                diameter_mm: Some(20.0),
                diameter_text: None,
                edge_hint: crate::Vec2::new(10.0, 0.0),
                ctrl_held: true,
            })
            .unwrap();
        manager
            .add_line(SegmentRequest {
                from: crate::Vec2::new(-12.0, 0.0),
                to_raw: crate::Vec2::new(12.0, 0.0),
                ctrl_held: true,
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let catalog = manager.profile_catalog();
        assert_eq!(catalog[0].profiles.len(), 2);
        assert!(catalog[0].profiles.iter().all(|profile| {
            profile.curves.len() == 2
                && profile
                    .curves
                    .iter()
                    .any(|curve| matches!(curve, ProfileCurveDto::Arc { .. }))
                && profile
                    .curves
                    .iter()
                    .any(|curve| matches!(curve, ProfileCurveDto::Line { .. }))
        }));

        for profile in &catalog[0].profiles {
            let plan = manager
                .prepare_extrude(ExtrudeRequest {
                    source_face: None,
                    sketch_name: "Sketch1".to_string(),
                    profile_indices: vec![profile.index],
                    operation: ExtrudeOperation::NewBody,
                    extent: ExtrudeExtent::Distance { distance: 4.0 },
                    taper_angle_deg: 0.0,
                    flip: false,
                    target_body_ids: Vec::new(),
                })
                .unwrap();
            let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
                panic!("semicircular region should plan an Extrude job");
            };
            assert_eq!(job.profiles[0].curves.len(), 2);
            manager.cancel_solid_recompute(plan.transaction_id);
        }
    }

    #[test]
    fn dimensioned_chain_with_an_attached_rectangle_keeps_its_closed_profile() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();

        manager
            .add_line_locked(LockedSegmentRequest {
                from: crate::Vec2::new(0.0, 0.0),
                to_hint: crate::Vec2::new(-15.0, 0.0),
                from_crossing: None,
                to_crossing: None,
                length_mm: None,
                angle_deg: None,
                length_text: Some("15".to_string()),
                angle_text: None,
                ctrl_held: false,
                tracking: None,
                intersection: None,
            })
            .unwrap();
        let vertical = manager
            .add_line_locked(LockedSegmentRequest {
                from: crate::Vec2::new(-15.0, 0.0),
                to_hint: crate::Vec2::new(-15.0, -7.5),
                from_crossing: None,
                to_crossing: None,
                length_mm: None,
                angle_deg: None,
                length_text: Some("7.5".to_string()),
                angle_text: None,
                ctrl_held: false,
                tracking: None,
                intersection: None,
            })
            .unwrap();
        assert_eq!(vertical.sketch.dimensions.len(), 2);
        manager
            .add_rectangle_locked(LockedRectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                anchor: crate::Vec2::new(-15.0, -7.5),
                width_mm: None,
                height_mm: None,
                width_text: Some("30".to_string()),
                height_text: Some("15".to_string()),
                corner_hint: crate::Vec2::new(15.0, 7.5),
                ctrl_held: false,
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let catalog = manager.profile_catalog();
        assert_eq!(catalog.len(), 1);
        assert_eq!(
            catalog[0].profiles.len(),
            1,
            "the open origin chain must not hide the attached rectangle"
        );
        assert!((catalog[0].profiles[0].area - 450.0).abs() < 1e-6);
        assert_eq!(
            catalog[0].profiles[0].curves.len(),
            4,
            "the longer rectangle carrier should remain one analytic edge"
        );
    }

    #[test]
    fn shared_edge_regions_are_two_extrudable_profiles() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();

        let a = crate::Vec2::new(0.0, 10.0);
        let b = crate::Vec2::new(10.0, 20.0);
        let c = crate::Vec2::new(20.0, 10.0);
        let d = crate::Vec2::new(0.0, 0.0);
        let e = crate::Vec2::new(20.0, 0.0);
        for (from, to) in [(a, b), (b, c), (c, a), (a, d), (d, e), (e, c)] {
            manager
                .add_line(SegmentRequest {
                    from,
                    to_raw: to,
                    ctrl_held: true,
                })
                .unwrap();
        }
        manager.end_sketch().unwrap();

        let catalog = manager.profile_catalog();
        let profiles = &catalog[0].profiles;
        assert_eq!(profiles.len(), 2);
        assert!(profiles
            .iter()
            .all(|profile| profile.parent_index.is_none() && profile.nesting_depth == 0));

        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: profiles.iter().map(|profile| profile.index).collect(),
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 10.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("shared-edge regions should plan an Extrude job");
        };
        assert_eq!(job.profiles.len(), 2);
        assert_eq!(job.result_body_ids.len(), 2);
    }

    #[test]
    fn endpoint_on_edge_junctions_create_two_analytic_extrude_profiles() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_rectangle(RectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                p1: crate::Vec2::new(0.0, 0.0),
                p2: crate::Vec2::new(40.0, 40.0),
                ctrl_held: false,
            })
            .unwrap();
        for (from, to) in [
            (crate::Vec2::new(20.0, 40.0), crate::Vec2::new(20.0, 20.0)),
            (crate::Vec2::new(20.0, 20.0), crate::Vec2::new(40.0, 20.0)),
        ] {
            manager
                .add_line(SegmentRequest {
                    from,
                    to_raw: to,
                    ctrl_held: true,
                })
                .unwrap();
        }
        manager.end_sketch().unwrap();

        let catalog = manager.profile_catalog();
        let profiles = &catalog[0].profiles;
        assert_eq!(profiles.len(), 2);
        assert!(profiles
            .iter()
            .all(|profile| profile.parent_index.is_none() && profile.nesting_depth == 0));
        assert!(
            profiles.iter().all(|profile| !profile.curves.is_empty()),
            "noded carrier pieces must retain their analytic line sources"
        );

        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: profiles.iter().map(|profile| profile.index).collect(),
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 10.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("endpoint-on-edge regions should plan an Extrude job");
        };
        assert_eq!(job.profiles.len(), 2);
        assert!(job
            .profiles
            .iter()
            .all(|profile| !profile.curves.is_empty()));
    }

    #[test]
    fn reference_project_requires_a_new_reader_and_preserves_measurement_semantics() {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        manager.begin_sketch(plane).unwrap();
        let line = manager
            .add_line(SegmentRequest {
                from: crate::Vec2::new(10.0, 10.0),
                to_raw: crate::Vec2::new(50.0, 10.0),
                ctrl_held: true,
            })
            .unwrap();
        let dimension = manager
            .add_dimension(DimensionRequest {
                entities: vec![line.entity_id],
                text_pos: crate::Vec2::new(30.0, 15.0),
                value_text: None,
            })
            .unwrap();
        let before = manager
            .set_dimension_mode(SetDimensionModeRequest {
                constraint_id: dimension.sketch.dimensions[0].constraint_id,
                mode: crate::DimensionMode::Reference,
            })
            .unwrap()
            .sketch;
        assert_eq!(before.dof.value, 4);
        manager.end_sketch().unwrap();
        let json = manager.export_project_model().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["schema_version"].as_u64().unwrap() > 2);

        for version in [2, PROJECT_SCHEMA_VERSION] {
            let mut input = parsed.clone();
            input["schema_version"] = version.into();
            if version < 11 {
                input.as_object_mut().unwrap().remove("print_intent");
            }
            let mut loaded = SketchManager::new();
            let plan = loaded.prepare_load_project(input.to_string()).unwrap();
            assert!(plan.jobs.is_empty());
            commit_plan(&mut loaded, plan, plane.origin_basis().unwrap());
            let after = loaded.edit_sketch("Sketch1").unwrap();
            assert_eq!(after.dof, before.dof);
            assert_eq!(after.dimensions.len(), 1);
            assert_eq!(after.dimensions[0].mode, crate::DimensionMode::Reference);
            assert_eq!(after.dimensions[0].param_id, None);
            assert!((after.dimensions[0].value - 40.0).abs() < 1e-9);
        }
    }

    #[test]
    fn schema_v2_driving_dimensions_migrate_without_changing_solver_or_parameters() {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        manager.begin_sketch(plane).unwrap();
        let line = manager
            .add_line(SegmentRequest {
                from: crate::Vec2::new(10.0, 10.0),
                to_raw: crate::Vec2::new(50.0, 10.0),
                ctrl_held: true,
            })
            .unwrap();
        let before = manager
            .add_dimension(DimensionRequest {
                entities: vec![line.entity_id],
                text_pos: crate::Vec2::new(30.0, 15.0),
                value_text: None,
            })
            .unwrap()
            .sketch;
        manager.end_sketch().unwrap();
        let mut legacy: serde_json::Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        legacy["schema_version"] = 2.into();
        legacy.as_object_mut().unwrap().remove("print_intent");
        legacy["sketches"][0]["snapshot"]
            .as_object_mut()
            .unwrap()
            .remove("dim_modes");

        let mut loaded = SketchManager::new();
        let plan = loaded.prepare_load_project(legacy.to_string()).unwrap();
        commit_plan(&mut loaded, plan, plane.origin_basis().unwrap());
        let resaved: serde_json::Value =
            serde_json::from_str(&loaded.export_project_model().unwrap()).unwrap();
        assert_eq!(resaved["schema_version"], PROJECT_SCHEMA_VERSION);
        let after = loaded.edit_sketch("Sketch1").unwrap();
        assert_eq!(after.dof, before.dof);
        assert_eq!(after.dimensions.len(), 1);
        assert_eq!(after.dimensions[0].mode, crate::DimensionMode::Driving);
        assert_eq!(after.dimensions[0].param_id, before.dimensions[0].param_id);
        assert_eq!(
            after.dimensions[0].param_name,
            before.dimensions[0].param_name
        );
        let edited = loaded
            .edit_dimension(EditDimensionRequest {
                constraint_id: after.dimensions[0].constraint_id,
                text: "55".to_string(),
            })
            .unwrap()
            .sketch;
        assert_eq!(edited.dof, before.dof);
        assert!((edited.dimensions[0].value - 55.0).abs() < 1e-7);
    }

    #[test]
    fn future_project_schema_is_rejected_without_replacing_document() {
        let mut manager = SketchManager::new();
        let error = manager
            .prepare_load_project(
                serde_json::json!({
                    "format": PROJECT_FORMAT,
                    "schema_version": PROJECT_SCHEMA_VERSION + 1
                })
                .to_string(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("not supported"));
        assert_eq!(manager.document().name(), "Untitled");
    }

    #[test]
    fn schema_v1_dimension_style_migrates_to_aligned() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let mut legacy: serde_json::Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        legacy["schema_version"] = serde_json::Value::from(1);
        legacy.as_object_mut().unwrap().remove("print_intent");
        legacy["document"]["settings"]["dimension_style"] =
            serde_json::Value::String("legacy_default".to_string());
        legacy["sketches"][0]["dimension_style"] =
            serde_json::Value::String("legacy_default".to_string());

        let migrated = decode_project(&legacy.to_string()).unwrap();
        assert_eq!(migrated.schema_version, PROJECT_SCHEMA_VERSION);
        assert_eq!(
            migrated.document.settings.dimension_style,
            DimensionStyle::Aligned
        );
        assert_eq!(
            migrated.sketches[0].dimension_style,
            DimensionStyle::Aligned
        );
    }

    #[test]
    fn pre_rename_project_label_is_normalized() {
        let manager = SketchManager::new();
        let mut legacy: serde_json::Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        legacy["format"] =
            serde_json::Value::String(crate::project::LEGACY_PROJECT_FORMAT.to_string());

        let migrated = decode_project(&legacy.to_string()).unwrap();
        assert_eq!(migrated.format, PROJECT_FORMAT);
        assert_eq!(migrated.schema_version, PROJECT_SCHEMA_VERSION);
    }

    #[test]
    fn unknown_project_label_is_rejected() {
        let manager = SketchManager::new();
        let mut unknown: serde_json::Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        unknown["format"] = serde_json::Value::String("unrelated-project".to_string());

        let error = decode_project(&unknown.to_string()).unwrap_err();
        assert!(error.contains("unsupported project format"));
    }

    #[test]
    fn project_roundtrip_preserves_sweep_loft_and_rib_definitions() {
        let mut manager = SketchManager::new();
        let xy = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let xz = PlaneRef::OriginPlane {
            plane: OriginPlane::Xz,
        };
        let basis = xy.origin_basis().unwrap();

        manager.begin_sketch(xy).unwrap();
        manager
            .add_rectangle_locked(LockedRectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                anchor: crate::Vec2::new(0.0, 0.0),
                width_mm: Some(20.0),
                height_mm: Some(10.0),
                width_text: Some("20".to_string()),
                height_text: Some("10".to_string()),
                corner_hint: crate::Vec2::new(20.0, 10.0),
                ctrl_held: false,
            })
            .unwrap();
        manager.end_sketch().unwrap();

        manager.begin_sketch(xz).unwrap();
        manager
            .add_rectangle_locked(LockedRectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                anchor: crate::Vec2::new(0.0, 0.0),
                width_mm: Some(10.0),
                height_mm: Some(10.0),
                width_text: Some("10".to_string()),
                height_text: Some("10".to_string()),
                corner_hint: crate::Vec2::new(10.0, 10.0),
                ctrl_held: false,
            })
            .unwrap();
        let path_line = manager
            .add_line(SegmentRequest {
                from: crate::Vec2::new(30.0, 0.0),
                to_raw: crate::Vec2::new(30.0, 30.0),
                ctrl_held: true,
            })
            .unwrap()
            .entity_id
            .0;
        manager.end_sketch().unwrap();

        let sweep = manager
            .prepare_sweep(SweepRequest {
                profile: ProfileRefDto {
                    sketch_name: "Sketch1".to_string(),
                    profile_index: 0,
                },
                path_sketch_name: "Sketch2".to_string(),
                path_entity_ids: vec![path_line],
                operation: ExtrudeOperation::NewBody,
                target_body_ids: Vec::new(),
                guide_rail: None,
                orientation: Default::default(),
                transition: Default::default(),
                force_c1: false,
            })
            .unwrap();
        commit_plan(&mut manager, sweep, basis);

        let loft = manager
            .prepare_loft(LoftRequest {
                sections: vec![
                    ProfileRefDto {
                        sketch_name: "Sketch1".to_string(),
                        profile_index: 0,
                    },
                    ProfileRefDto {
                        sketch_name: "Sketch2".to_string(),
                        profile_index: 0,
                    },
                ],
                ruled: false,
                operation: ExtrudeOperation::NewBody,
                target_body_ids: Vec::new(),
                continuity: Default::default(),
                centerline: None,
                guide_rail: None,
            })
            .unwrap();
        commit_plan(&mut manager, loft, basis);

        let rib = manager
            .prepare_rib(RibRequest {
                sketch_name: "Sketch2".to_string(),
                line_entity_ids: vec![path_line],
                thickness: 2.0,
                depth: 8.0,
                symmetric: true,
                flip: false,
                operation: ExtrudeOperation::NewBody,
                target_body_ids: Vec::new(),
                extent: None,
            })
            .unwrap();
        commit_plan(&mut manager, rib, basis);

        let json = manager.export_project_model().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["sweeps"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["lofts"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["ribs"].as_array().unwrap().len(), 1);

        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        assert_eq!(replay.jobs.len(), 3);
        let replay_ids = replay
            .jobs
            .iter()
            .flat_map(result_body_ids)
            .copied()
            .collect::<BTreeSet<_>>();
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: replay.transaction_id,
                scene: KernelSceneDto {
                    bodies: replay_ids
                        .into_iter()
                        .map(|id| raw_body(id, basis))
                        .collect(),
                    errors: Vec::new(),
                },
            })
            .unwrap();
        assert_eq!(loaded.sweep_definitions().len(), 1);
        assert_eq!(loaded.loft_definitions().len(), 1);
        assert_eq!(loaded.rib_definitions().len(), 1);
    }

    #[test]
    fn project_roundtrip_preserves_edge_refinements_and_hole_definitions() {
        let mut manager = SketchManager::new();
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let basis = plane.origin_basis().unwrap();
        manager.begin_sketch(plane).unwrap();
        manager
            .add_rectangle(RectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                p1: crate::Vec2::new(0.0, 0.0),
                p2: crate::Vec2::new(20.0, 10.0),
                ctrl_held: false,
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let extrude = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 10.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        commit_plan(&mut manager, extrude, basis);
        let body = &manager.solid_scene().bodies[0];
        let body_id = body.id;
        let first_edge = body.edges[0].id;
        let second_edge = body.edges[1].id;
        let face_id = body.faces[0].id;

        let fillet = manager
            .prepare_solid_fillet(SolidFilletRequest {
                body_id,
                edge_ids: vec![first_edge],
                radius: 1.0,
                tangent_chain: false,
            })
            .unwrap();
        commit_plan(&mut manager, fillet, basis);
        let chamfer = manager
            .prepare_solid_chamfer(SolidChamferRequest {
                body_id,
                edge_ids: vec![second_edge],
                distance: 0.5,
                tangent_chain: false,
            })
            .unwrap();
        commit_plan(&mut manager, chamfer, basis);
        let hole = manager
            .prepare_hole(HoleRequest {
                body_id,
                face_id,
                position: Point2Dto::new(5.0, 5.0),
                position_reference: None,
                positions: Vec::new(),
                diameter: 2.5,
                extent: HoleExtent::ThroughAll,
                style: HoleStyle::Countersink,
                counterbore_diameter: 0.0,
                counterbore_depth: 0.0,
                countersink_diameter: 4.0,
                countersink_angle_deg: 90.0,
                bottom_style: limo_cad_solid::HoleBottomStyle::Flat,
                drill_point_angle_deg: 118.0,
                thread: Some(limo_cad_solid::HoleThreadDto {
                    standard: limo_cad_solid::HoleThreadStandard::IsoMetric,
                    series: limo_cad_solid::HoleThreadSeries::MetricCoarse,
                    designation: "M3 x 0.5 - 6H".to_string(),
                    class: "6H".to_string(),
                    nominal_diameter: 3.0,
                    pitch: 0.5,
                    threads_per_inch: None,
                    hand: limo_cad_solid::HoleThreadHand::Right,
                    depth: None,
                    representation: limo_cad_solid::HoleThreadRepresentation::Modeled,
                    tap_drill_designation: Some("2.5 mm".to_string()),
                    rounded_profile: None,
                }),
                flip: false,
            })
            .unwrap();
        commit_plan(&mut manager, hole, basis);

        let json = manager.export_project_model().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["fillets"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["chamfers"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["holes"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["holes"][0]["thread"]["designation"], "M3 x 0.5 - 6H");

        let mut loaded = SketchManager::new();
        let replay = loaded.prepare_load_project(json).unwrap();
        assert!(matches!(replay.jobs[1], KernelJobDto::Fillet(_)));
        assert!(matches!(replay.jobs[2], KernelJobDto::Chamfer(_)));
        assert!(matches!(replay.jobs[3], KernelJobDto::Hole(_)));
        commit_plan(&mut loaded, replay, basis);
        assert_eq!(loaded.fillet_definitions().len(), 1);
        assert_eq!(loaded.chamfer_definitions().len(), 1);
        assert_eq!(loaded.hole_definitions().len(), 1);
        assert_eq!(
            loaded.hole_definitions()[0]
                .thread
                .as_ref()
                .map(|thread| thread.class.as_str()),
            Some("6H")
        );
    }

    #[test]
    fn fillet_profile_preserves_one_analytic_arc_for_the_kernel() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_rectangle(RectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                p1: crate::Vec2::new(0.0, 0.0),
                p2: crate::Vec2::new(30.0, 20.0),
                ctrl_held: false,
            })
            .unwrap();
        let dto = manager.active_snapshot().unwrap();
        let bottom = dto
            .entities
            .iter()
            .find_map(|entity| match entity {
                crate::dto::EntityDto::Line { id, start, end, .. }
                    if start.y.abs() < 1e-8 && end.y.abs() < 1e-8 =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .unwrap();
        let left = dto
            .entities
            .iter()
            .find_map(|entity| match entity {
                crate::dto::EntityDto::Line { id, start, end, .. }
                    if start.x.abs() < 1e-8 && end.x.abs() < 1e-8 =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .unwrap();
        manager
            .fillet_lines(FilletRequest {
                l1: bottom,
                l2: left,
                radius_text: "5".to_string(),
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let catalog = manager.profile_catalog();
        let profile = &catalog[0].profiles[0];
        assert!(profile.points.len() > profile.curves.len());
        assert_eq!(
            profile
                .curves
                .iter()
                .filter(|curve| matches!(curve, ProfileCurveDto::Arc { .. }))
                .count(),
            1
        );

        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 10.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("expected extrude job");
        };
        assert_eq!(
            job.profiles[0]
                .curves
                .iter()
                .filter(|curve| matches!(curve, KernelCurveDto::Arc { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn exact_adjacent_fillets_omit_the_consumed_carrier_from_solid_topology() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_rectangle(RectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                p1: crate::Vec2::new(0.0, 0.0),
                p2: crate::Vec2::new(30.0, 30.0),
                ctrl_held: false,
            })
            .unwrap();
        let dto = manager.active_snapshot().unwrap();
        let horizontal = |y: f64| {
            dto.entities.iter().find_map(|entity| match entity {
                crate::dto::EntityDto::Line { id, start, end, .. }
                    if (start.y - y).abs() < 1e-8 && (end.y - y).abs() < 1e-8 =>
                {
                    Some(*id)
                }
                _ => None,
            })
        };
        let vertical = |x: f64| {
            dto.entities.iter().find_map(|entity| match entity {
                crate::dto::EntityDto::Line { id, start, end, .. }
                    if (start.x - x).abs() < 1e-8 && (end.x - x).abs() < 1e-8 =>
                {
                    Some(*id)
                }
                _ => None,
            })
        };
        let bottom = horizontal(0.0).unwrap();
        let left = vertical(0.0).unwrap();
        let right = vertical(30.0).unwrap();
        manager
            .fillet_lines(FilletRequest {
                l1: bottom,
                l2: left,
                radius_text: "15".to_string(),
            })
            .unwrap();
        manager
            .fillet_lines(FilletRequest {
                l1: bottom,
                l2: right,
                radius_text: "15".to_string(),
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let catalog = manager.profile_catalog();
        let profile = &catalog[0].profiles[0];
        assert_eq!(profile.curves.len(), 4, "top + two sides + one semicircle");
        assert_eq!(
            profile
                .curves
                .iter()
                .filter(|curve| matches!(curve, ProfileCurveDto::Arc { .. }))
                .count(),
            1
        );
        assert!(profile.curves.iter().all(|curve| match curve {
            ProfileCurveDto::Line { start, end, .. } => point2_distance(*start, *end) > 1e-3,
            _ => true,
        }));

        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".to_string(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 10.0 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: Vec::new(),
            })
            .unwrap();
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("expected extrude job");
        };
        assert_eq!(
            job.profiles[0]
                .curves
                .iter()
                .filter(|curve| matches!(curve, KernelCurveDto::Arc { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn adjacent_fillets_below_the_limit_keep_their_real_carrier() {
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_rectangle(RectangleRequest {
                mode: crate::dto::RectangleMode::TwoPoint,
                p1: crate::Vec2::new(0.0, 0.0),
                p2: crate::Vec2::new(30.0, 30.0),
                ctrl_held: false,
            })
            .unwrap();
        let dto = manager.active_snapshot().unwrap();
        let horizontal = |y: f64| {
            dto.entities.iter().find_map(|entity| match entity {
                crate::dto::EntityDto::Line { id, start, end, .. }
                    if (start.y - y).abs() < 1e-8 && (end.y - y).abs() < 1e-8 =>
                {
                    Some(*id)
                }
                _ => None,
            })
        };
        let vertical = |x: f64| {
            dto.entities.iter().find_map(|entity| match entity {
                crate::dto::EntityDto::Line { id, start, end, .. }
                    if (start.x - x).abs() < 1e-8 && (end.x - x).abs() < 1e-8 =>
                {
                    Some(*id)
                }
                _ => None,
            })
        };
        let bottom = horizontal(0.0).unwrap();
        manager
            .fillet_lines(FilletRequest {
                l1: bottom,
                l2: vertical(0.0).unwrap(),
                radius_text: "14".to_string(),
            })
            .unwrap();
        manager
            .fillet_lines(FilletRequest {
                l1: bottom,
                l2: vertical(30.0).unwrap(),
                radius_text: "14".to_string(),
            })
            .unwrap();
        manager.end_sketch().unwrap();

        let profile = &manager.profile_catalog()[0].profiles[0];
        assert_eq!(profile.curves.len(), 6);
        assert_eq!(
            profile
                .curves
                .iter()
                .filter(|curve| matches!(curve, ProfileCurveDto::Arc { .. }))
                .count(),
            2
        );
        assert!(profile.curves.iter().any(|curve| matches!(
            curve,
            ProfileCurveDto::Line { start, end, .. }
                if (point2_distance(*start, *end) - 2.0).abs() < 1e-3
        )));
    }

    #[test]
    fn intentional_tiny_untrimmed_edges_are_not_classified_as_consumed() {
        let sketch = SketchDto {
            name: "Tiny".to_string(),
            edit_occurrence_id: None,
            plane: PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            },
            basis: PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            }
            .origin_basis()
            .unwrap(),
            entities: vec![
                crate::dto::EntityDto::Line {
                    id: EntityId(1),
                    start_id: EntityId(10),
                    end_id: EntityId(11),
                    start: crate::Vec2::new(0.0, 0.0),
                    end: crate::Vec2::new(0.0005, 0.0),
                    fully_defined: true,
                    consumed: false,
                },
                crate::dto::EntityDto::Line {
                    id: EntityId(2),
                    start_id: EntityId(11),
                    end_id: EntityId(12),
                    start: crate::Vec2::new(0.0005, 0.0),
                    end: crate::Vec2::new(0.0005, 1.0),
                    fully_defined: true,
                    consumed: false,
                },
                crate::dto::EntityDto::Line {
                    id: EntityId(3),
                    start_id: EntityId(12),
                    end_id: EntityId(13),
                    start: crate::Vec2::new(0.0005, 1.0),
                    end: crate::Vec2::new(0.0, 1.0),
                    fully_defined: true,
                    consumed: false,
                },
                crate::dto::EntityDto::Line {
                    id: EntityId(4),
                    start_id: EntityId(13),
                    end_id: EntityId(10),
                    start: crate::Vec2::new(0.0, 1.0),
                    end: crate::Vec2::new(0.0, 0.0),
                    fully_defined: true,
                    consumed: false,
                },
            ],
            constraints: Vec::new(),
            reference_midpoints: Vec::new(),
            dimensions: Vec::new(),
            dimension_style: DimensionStyle::Aligned,
            grid_snap: true,
            dof: crate::dto::DofDto {
                value: 0,
                fully_defined: true,
            },
            can_undo: false,
            can_redo: false,
            projected_edges: Vec::new(),
        };

        assert!(consumed_trim_carrier_ids(&sketch, 1e-3).is_empty());
        let catalog = profile_catalog_item(&sketch, FeatureId(1));
        assert_eq!(catalog.profiles.len(), 1);
        assert!(catalog.profiles[0].area > 0.00049);
    }

    #[test]
    fn construction_planes_propagate_edits_and_reject_self_references() {
        let mut manager = SketchManager::new();
        let offset = manager
            .create_datum_plane(DatumPlaneRequest {
                name: None,
                source: DatumPlaneSourceDto::Offset {
                    reference: PlaneRef::OriginPlane {
                        plane: OriginPlane::Xy,
                    },
                    distance: 20.0,
                },
            })
            .unwrap();
        let offset_definition = offset.planes[0].clone();
        assert!((offset_definition.basis.origin[2] - 20.0).abs() < 1e-9);

        let midplane = manager
            .create_datum_plane(DatumPlaneRequest {
                name: None,
                source: DatumPlaneSourceDto::Midplane {
                    first: PlaneRef::OriginPlane {
                        plane: OriginPlane::Xy,
                    },
                    second: PlaneRef::DatumPlane {
                        datum_id: offset_definition.datum_id,
                    },
                },
            })
            .unwrap();
        assert!((midplane.planes[1].basis.origin[2] - 10.0).abs() < 1e-9);

        let edited = manager
            .edit_datum_plane(EditDatumPlaneRequest {
                feature_id: offset_definition.feature_id,
                plane: DatumPlaneRequest {
                    name: None,
                    source: DatumPlaneSourceDto::Offset {
                        reference: PlaneRef::OriginPlane {
                            plane: OriginPlane::Xy,
                        },
                        distance: 40.0,
                    },
                },
            })
            .unwrap();
        assert!((edited.planes[0].basis.origin[2] - 40.0).abs() < 1e-9);
        assert!((edited.planes[1].basis.origin[2] - 20.0).abs() < 1e-9);

        let self_reference = manager.edit_datum_plane(EditDatumPlaneRequest {
            feature_id: offset_definition.feature_id,
            plane: DatumPlaneRequest {
                name: None,
                source: DatumPlaneSourceDto::Offset {
                    reference: PlaneRef::DatumPlane {
                        datum_id: offset_definition.datum_id,
                    },
                    distance: 5.0,
                },
            },
        });
        assert!(matches!(
            self_reference,
            Err(SessionError::BrokenReference(message))
                if message.contains("missing or rolled back")
        ));
    }

    #[test]
    fn plane_at_angle_can_replay_from_cached_straight_edge_endpoints() {
        let mut manager = SketchManager::new();
        let result = manager
            .create_datum_plane(DatumPlaneRequest {
                name: None,
                source: DatumPlaneSourceDto::AtAngle {
                    reference: PlaneRef::OriginPlane {
                        plane: OriginPlane::Xy,
                    },
                    body_id: BodyId(99),
                    edge_id: limo_cad_core::EdgeId(101),
                    angle_deg: 90.0,
                    axis_points: Some([
                        Point3Dto::from([0.0, 0.0, 0.0]),
                        Point3Dto::from([10.0, 0.0, 0.0]),
                    ]),
                },
            })
            .unwrap();
        let normal = result.planes[0].basis.normal;
        assert!(normal[0].abs() < 1e-9);
        assert!((normal[1] + 1.0).abs() < 1e-9);
        assert!(normal[2].abs() < 1e-9);
    }
}
