//! limo-cad-sketch — pure-Rust 2D sketch model and session API.
//!
//! - Sketch entities: point, line (endpoints are shared point entities —
//!   structural coincident), arc, circle.
//! - Constraints: the 12 geometric constraints of M1 plus Fix/Unfix and the
//!   dimensional constraints (distance, radius, diameter, angle).
//! - [`solver`]: Newton-based constraint solver with real DOF tracking and
//!   over-constraint rejection.
//! - [`SketchSession`]: drawing behavior — snap, auto-constraint inference,
//!   solver-pinned rubber-band dragging, locked dynamic input, tool ops,
//!   and undo/redo.
//! - [`SketchManager`]: document + session lifecycle, held by both engine
//!   hosts; [`host::handle`] is the shared JSON dispatch both hosts use.
//!
//! This crate never touches OCCT — the sketch solver is pure Rust.

mod cam_chamfer;
mod cam_height_geometry;
pub use cam_height_geometry::resolve as resolve_cam_height_geometry;
mod constraint;
mod drawing;
pub mod drawing_commands;
mod drawing_layout;
mod drawing_references;
pub use drawing_layout::update_drawing_view;
pub mod drawing_topology;
mod dto;
mod edge_selection;
mod entity;
mod expr;
mod geometry;
mod geomops;
mod manager;
mod named_view_history;
pub use named_view_history::normalize_named_view_history_ids;
mod params;
mod plane;
mod profile_identity;
mod project;
pub use project::PROJECT_SCHEMA_VERSION;
mod session;
mod sketch;
mod solver;

pub mod host;

pub use cam_chamfer::{
    resolve as resolve_cam_chamfer_geometry, CamChamferGeometry, CamChamferGeometryRequest,
};
pub use constraint::{ArcEndpoint, Constraint, ConstraintId, ConstraintKind};
pub use drawing::{
    DrawingAnnotationDto, DrawingAttachmentRefDto, DrawingBomItemDto, DrawingBreakAxis,
    DrawingChainDimensionLayout, DrawingCircularRefDto, DrawingDatumReferenceDto,
    DrawingDimensionPresentationDto, DrawingDimensionToleranceDto, DrawingDimensionToleranceMode,
    DrawingDocumentDto, DrawingDualUnitDto, DrawingDualUnitPlacement, DrawingEdgeEndpoint,
    DrawingGdtCharacteristic, DrawingHoleStyle, DrawingLineDimensionMode, DrawingLineRefDto,
    DrawingLineStyleDto, DrawingLinearDimensionMode, DrawingMaterialCondition, DrawingOrdinateAxis,
    DrawingProjectionMethod, DrawingRadialDimensionMode, DrawingReleaseDto, DrawingReleaseStatus,
    DrawingRevisionDto, DrawingSecondaryUnit, DrawingSheetDto, DrawingSheetFormat,
    DrawingSheetOrientation, DrawingSheetStyleDto, DrawingStandard, DrawingSurfaceLay,
    DrawingTemplateDto, DrawingTitleBlockDto, DrawingToleranceNoteDto, DrawingTolerancePreset,
    DrawingTopologyAnchorRefDto, DrawingViewAlignment, DrawingViewDerivationDto, DrawingViewDto,
    DrawingViewKind, DrawingViewScope, DrawingWeldContour, DrawingWeldSide, DrawingWeldType,
};
pub use dto::{
    err_json, ok_json, AddConstraintResult, AddLineResult, Arc3PointRequest, ArcCenterRequest,
    BeginSketchRequest, BreakRequest, ChamferRequest, CircleMode, CircleRequest,
    CircularPatternRequest, ConstraintBatchRequest, ConstraintDesc, ConstraintDto,
    ConstructionVisibilityRequest, CreationPointPreviewRequest, CreationPreviewDto,
    CreationPreviewRequest, CurveCrossingRequest, DeleteConstraintRequest, DeleteDimensionRequest,
    DeleteEntitiesRequest, DeleteEntityRequest, DeleteEntityResult, DimensionDto, DimensionRequest,
    DofDto, DragPhase, EditDimensionRequest, EditSketchRequest, EndSketchResult, EntityDesc,
    EntityDto, EvalExpressionRequest, EvalExpressionResult, ExtendRequest, FaceSketchOrigin,
    FilletPreviewDto, FilletRequest, Inference, LineIntersectionRequest, LineTrackingRequest,
    LockedCircleRequest, LockedRectangleRequest, LockedSegmentRequest, MidpointLineRequest,
    MirrorRequest, MoveCopyRequest, MoveDimensionRequest, MovePointRequest, MovePointResult,
    NamedViewConfigurationDto, NamedViewsDto, OffsetPreviewDto, OffsetRequest, PointRequest,
    PolygonRequest, PreviewCurve, PreviewDto, ProjectVisibilityDto, ProjectedCircleDto,
    ProjectedEdgeDto, RecallNamedViewDto, RectangleMode, RectangleRequest,
    RectangularPatternRequest, ReferenceMidpointDto, ScaleRequest, SegmentRequest,
    SetDimensionModeRequest, SetDimensionStyleRequest, SetGridSnapRequest, SetGridStepRequest,
    SketchDto, SlotMode, SlotRequest, SnapTarget, SplineRequest, ToggleFixBatchRequest, ToolResult,
    TrackingAxis, TrackingGuideDto, TrimPreviewDto, TrimRequest, UndoResult, ViewCameraDto,
    ViewPartOffsetDto, ViewportSnapContext,
};
pub use edge_selection::{
    candidates as edge_chain_candidates, resolve as resolve_edge_chain, ChainMode, ChainSource,
    EdgeChainRequest,
};
pub use entity::{Entity, EntityId};
pub use expr::{
    eval_expression, parse as parse_expression, referenced_idents, Ast, ExprError,
    Func as ExpressionFunction, Op as ExpressionOperator,
};
pub use geometry::Vec2;
pub use manager::resolve_cam_hole as resolve_cam_hole_reference;

pub use geomops::{slot::slot_capsule, spline::tessellate_spline};
pub use limo_cad_assembly::{
    approximate_pair_result, broad_phase_interference_pairs, contact_violation_score,
    ApplyJointMotionsRequestDto, AssemblyDiagnosticDto, AssemblyDiagnosticKindDto,
    AssemblyDocumentDto, AssemblyPositionDto, AssemblyPositionId, AssemblySolutionDto,
    AssemblyTransformDto, BodyPoseDto, ComponentDefinitionDto, ComponentDefinitionPatchDto,
    ComponentId, ComponentOccurrenceDto, ComponentOccurrencePatchDto, ComponentStructureDto,
    ContactSetDto, ContactSetId, CreateAssemblyPositionRequestDto, CreateComponentRequestDto,
    CreateContactSetRequestDto, CreateGearRelationRequestDto, CreateJointRequestDto,
    CreateMotionStudyRequestDto, CreateOccurrenceRequestDto, DuplicateOccurrenceRequestDto,
    EvaluateMotionStudyRequestDto, GearRelationDto, InstanceBodyPoseDto,
    InterferenceCheckRequestDto, InterferencePairResultDto, InterferenceReportDto,
    JointAdvancedDto, JointConnectorDto, JointDefinitionDto, JointFrameDto, JointId, JointKindDto,
    JointLimitsDto, JointMotionStateDto, MechanismDragRequestDto, MechanismPreviewDto,
    MotionCoordinateDto, MotionDriverDto, MotionDriverId, MotionDriverLawDto,
    MotionDriverSampleDto, MotionInterpolationDto, MotionKeyframeDto, MotionPathRequestDto,
    MotionStudyDto, MotionStudyEvaluationDto, MotionStudyId, MotionStudySampleDto, OccurrenceId,
    OccurrencePoseDto, RemoveOccurrenceRequestDto, SampleMotionStudyRequestDto,
    SetJointCoordinatesRequestDto, SetJointEnabledRequestDto, SetJointMotionRequestDto,
    SetOccurrenceGroundedRequestDto, SetOccurrencePoseRequestDto, SweptCollisionEventDto,
    SweptCollisionReportDto, SweptCollisionRequestDto, UpdateComponentRequestDto,
    UpdateJointRequestDto, UpdateOccurrenceRequestDto, ViewOccurrenceOffsetDto,
};
pub use limo_cad_cam::{
    BoxAnchor, CamArcPlane, CamCommandDto, CamDocumentDto, CamGcodeDialectDto,
    CamGcodeSimulationRequestDto, CamOperationDto, CamPostConfigDto, CamPostRequestDto,
    CamPostResultDto, CamProgramDto, CamProgramStatsDto, CamResolvedStockDto, CamSetupDto,
    CamSimulationCollisionDto, CamSimulationMeshDto, CamSimulationRequestDto,
    CamSimulationResultDto, CamSimulationSourceDto, CamSimulationStepDto, CamSimulationStepKind,
    CamStockFace, CamStockMeshDto, CamStockOffsetsDto, CamStockPlacementDto, CamStockShape,
    CamStockSpecDto, CamToolDto, CamToolKind, CamUnits, ContourCompensation, CoolantMode,
    CuttingParametersDto, MotionKind, NbPostAnalysisDto, NbPostAnalysisRequestDto,
    NbPostCompatibilityLevel, NbPostSourceKind, Point2Dto as CamPoint2Dto,
    Point3Dto as CamPoint3Dto, PostDialect, PostEventDto, PostEventStreamDto,
    Rect2Dto as CamRect2Dto, Siemens828dAtcStyle, Siemens828dPostConfigDto,
    Siemens828dToolChangePositioning, SpindleDirection, StockBoxDto, WcsOriginSpecDto,
    WorkCoordinateSystemDto, WorkOffset,
};
pub use manager::{construction_plane_basis, RetainedSketchSessions, SketchManager};
pub use params::{ParamId, ParamKind, ParamTable, Parameter};
pub use plane::{FaceId, OriginPlane, PlaneBasis, PlaneError, PlaneRef};
pub use session::{SessionError, SketchSession};
pub use sketch::{DimensionMode, DofReport, Sketch, SketchSnapshot, SolveError};
pub use solver::Analysis;
