//! Host-neutral 3-axis CAM foundation.
//!
//! The persistent job model stores manufacturing intent: setups, WCS/stock,
//! tools, and operations. [`plan_setup`] deterministically expands that intent
//! into controller-neutral motion. Setup machine snapshots bind resources and
//! constrain NC output; controller words remain in posts, not geometry code.
//! Rotary and multi-channel resources can be stored but are not executable yet.

mod cutter;
mod gcode;
pub use cutter::{
    cutter_mesh, CamCornerChamferDto, CamCutterGeometryDto, CamCutterMeshDto, CamCutterMeshPartDto,
    CutterProfile,
};
mod dependencies;
pub use dependencies::{
    cam_operation_dependencies, CamOperationDependency, CamOperationDependencyKind,
    CAM_ORDER_DEPENDENCY_RULES_REVISION,
};
mod machine;
mod model;
pub use machine::*;
mod compensation;
mod tool_calls;
pub use tool_calls::{CamMachineToolBindingDto, CamMachineToolCallDto};
mod linking;
pub use linking::*;
mod nbpost;
mod planner;
mod post;
mod post_events;
mod simulation;

#[cfg(test)]
mod stabilization_tests;

#[cfg(test)]
mod lead_regression_tests;

pub use gcode::{
    simulate_gcode, simulate_gcode_with_cancellation, CamGcodeDialectDto,
    CamGcodeSimulationRequestDto, MAX_GCODE_BYTES,
};
pub use model::{
    BoxAnchor, CamAdaptiveGeometryDto, CamAdaptiveParametersDto, CamChainRefDto, CamChainSource,
    CamChamferChainDto, CamCuttingPresetDto, CamDocumentDto, CamFlatParametersDto,
    CamHeightExpressionDto, CamHeightGeometryDto, CamHeightReferenceDto, CamHoleDto,
    CamLoadWarningDto, CamModeledChamferDto, CamOperationDto, CamOperationHeightExpressionsDto,
    CamPostConfigDto, CamResolvedStockDto, CamSetupDto, CamStockFace, CamStockOffsetsDto,
    CamStockPlacementDto, CamStockShape, CamStockSpecDto, CamToolCallMode, CamToolDto, CamToolKind,
    CamToolpathGenerationDto, CamToolpathOrderDependenciesDto, CamToolpathStateDto,
    CamToolpathStatusDto, CamUnits, CompensationMode, ContourCompensation, CoolantMode,
    CuttingParametersDto, DrillCycle, FaceDirection, MillingDirection, Point2Dto, Point3Dto,
    PostDialect, Rect2Dto, Siemens828dAtcStyle, Siemens828dPostConfigDto,
    Siemens828dToolChangePositioning, SpindleDirection, StockBoxDto, WcsOriginSpecDto,
    WorkCoordinateSystemDto, WorkOffset,
};
pub use nbpost::{
    analyze_nbpost, NbPostAnalysisDto, NbPostAnalysisRequestDto, NbPostCompatibilityLevel,
    NbPostSourceKind,
};
pub use planner::{
    plan_setup, plan_setup_through, CamArcPlane, CamCommandDto, CamPlanError, CamProgramDto,
    CamProgramStatsDto, MotionKind,
};
pub use post::{post_setup, CamPostRequestDto, CamPostResultDto};
pub use post_events::{post_event_stream, PostEventDto, PostEventStreamDto};
pub use simulation::{
    simulate_setup, simulate_setup_with_cancellation, CamPlayback, CamSimulationCancellation,
    CamSimulationCollisionDto, CamSimulationCollisionKindDto, CamSimulationComparisonDto,
    CamSimulationMeshDto, CamSimulationRequestDto, CamSimulationResultDto, CamSimulationSourceDto,
    CamSimulationStepDto, CamSimulationStepKind, CamSimulationTargetDto, CamStockMeshDto,
};
