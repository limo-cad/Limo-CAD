use nbcad_core::{BodyId, DocumentDto, EdgeId, FaceId, FeatureId, PlaneBasis, PlaneRef};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point2Dto {
    pub x: f64,
    pub y: f64,
}

impl Point2Dto {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point3Dto {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl From<[f64; 3]> for Point3Dto {
    fn from(value: [f64; 3]) -> Self {
        Self {
            x: value[0],
            y: value[1],
            z: value[2],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileLoopDto {
    pub index: u32,
    pub points: Vec<Point2Dto>,
    pub area: f64,
    /// Smallest enclosing loop, when this loop is nested. Even nesting depths
    /// are material regions; odd depths are holes in their parent region.
    #[serde(default)]
    pub parent_index: Option<u32>,
    #[serde(default)]
    pub nesting_depth: u32,
    /// Ordered analytic sketch curves. `points` remains the deterministic
    /// tessellation used for profile discovery, hit-testing, and migration;
    /// the solid kernel consumes these curves so arcs do not become chords.
    #[serde(default)]
    pub curves: Vec<ProfileCurveDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileCurveDto {
    Line {
        entity_id: u64,
        /// Every editable sketch entity represented by this derived boundary
        /// curve. A canonical line can replace several adjacent collinear
        /// entities without losing the design-intent provenance.
        #[serde(default)]
        source_entity_ids: Vec<u64>,
        start: Point2Dto,
        end: Point2Dto,
    },
    /// A circular arc oriented from `start` through `mid` to `end`.
    Arc {
        entity_id: u64,
        /// Source sketch entities merged into this analytic boundary arc.
        /// For example, two limiting R10 fillets can become one semicircle
        /// for the solid kernel while remaining two editable sketch fillets.
        #[serde(default)]
        source_entity_ids: Vec<u64>,
        start: Point2Dto,
        mid: Point2Dto,
        end: Point2Dto,
    },
    /// One closed analytic circle edge.
    Circle {
        entity_id: u64,
        /// Source sketch entities represented by this derived full circle.
        #[serde(default)]
        source_entity_ids: Vec<u64>,
        center: Point2Dto,
        radius: f64,
    },
    /// Spline fallback until native B-spline control data is carried through.
    Polyline {
        entity_id: u64,
        /// Source sketch entities represented by this simplified fallback.
        #[serde(default)]
        source_entity_ids: Vec<u64>,
        points: Vec<Point2Dto>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileCatalogItemDto {
    pub sketch_name: String,
    pub feature_id: FeatureId,
    pub basis: PlaneBasis,
    pub profiles: Vec<ProfileLoopDto>,
    /// When no bounded face was found, retain the specific topology reason
    /// instead of collapsing every failure into an indistinguishable empty
    /// catalog. Solid dialogs can then tell users whether geometry is open,
    /// degenerate, branching, or self-intersecting.
    #[serde(default)]
    pub profile_error: Option<String>,
    /// Stable straight-line entities available as references for axes,
    /// sweep paths, and rib centerlines. Curves can join this catalog later
    /// without changing the persisted feature reference shape.
    #[serde(default)]
    pub lines: Vec<SketchLineDto>,
    /// Stable analytic/open sketch curves available to path-driven features.
    #[serde(default)]
    pub path_curves: Vec<SketchPathCurveDto>,
    /// Stable, named points that downstream features can reference
    /// associatively (for example a Hole centered on a line endpoint).
    #[serde(default)]
    pub reference_points: Vec<SketchReferencePointDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchLineDto {
    pub entity_id: u64,
    pub start: Point2Dto,
    pub end: Point2Dto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchPathCurveDto {
    Line {
        entity_id: u64,
        start: Point2Dto,
        end: Point2Dto,
    },
    Arc {
        entity_id: u64,
        start: Point2Dto,
        mid: Point2Dto,
        end: Point2Dto,
    },
    Circle {
        entity_id: u64,
        center: Point2Dto,
        radius: f64,
    },
    Spline {
        entity_id: u64,
        points: Vec<Point2Dto>,
    },
}

impl SketchPathCurveDto {
    pub fn entity_id(&self) -> u64 {
        match self {
            Self::Line { entity_id, .. }
            | Self::Arc { entity_id, .. }
            | Self::Circle { entity_id, .. }
            | Self::Spline { entity_id, .. } => *entity_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchPointKindDto {
    Point,
    Start,
    End,
    Center,
    FitPoint { index: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchReferencePointDto {
    pub entity_id: u64,
    #[serde(flatten)]
    pub point: SketchPointKindDto,
    pub position: Point2Dto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SketchPointRefDto {
    pub sketch_name: String,
    pub entity_id: u64,
    #[serde(flatten)]
    pub point: SketchPointKindDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtrudeOperation {
    NewBody,
    Join,
    Cut,
    Intersect,
}

impl Default for ExtrudeOperation {
    fn default() -> Self {
        Self::NewBody
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtrudeExtent {
    Distance { distance: f64 },
    TwoSides { distance: f64, second_distance: f64 },
    Symmetric { distance: f64 },
    ThroughAll,
    ToFace { face_id: FaceId },
}

impl Default for ExtrudeExtent {
    fn default() -> Self {
        Self::Distance { distance: 10.0 }
    }
}

/// Stable application-level reference to one exact planar OCCT face.
///
/// The request deliberately carries the owning body as well as the FaceId:
/// FaceId is stable for the body-local topology key, while the body id tells
/// the kernel which live B-rep owns that topology during a full replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanarFaceSourceDto {
    pub body_id: BodyId,
    pub face_id: FaceId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeRequest {
    /// When present, Extrude consumes the referenced OCCT face instead of
    /// sketch profiles. The exact TopoDS_Face (including every inner wire) is
    /// resolved by the kernel; tessellation is never used as modeling input.
    #[serde(default)]
    pub source_face: Option<PlanarFaceSourceDto>,
    pub sketch_name: String,
    pub profile_indices: Vec<u32>,
    #[serde(default)]
    pub operation: ExtrudeOperation,
    #[serde(default)]
    pub extent: ExtrudeExtent,
    #[serde(default)]
    pub taper_angle_deg: f64,
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub target_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditExtrudeRequest {
    pub feature_id: FeatureId,
    pub extrude: ExtrudeRequest,
}

/// Create one or more New Body solids by rotating closed sketch profiles
/// around an axis expressed in the sketch's local 2D coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevolveRequest {
    pub sketch_name: String,
    pub profile_indices: Vec<u32>,
    pub axis_origin: Point2Dto,
    pub axis_direction: Point2Dto,
    /// Sketch that owns `axis_line_entity_id`. Legacy definitions omit this
    /// and resolve the line from `sketch_name`.
    #[serde(default)]
    pub axis_line_sketch_name: Option<String>,
    /// When present, the stable line entity replaces the manual axis values.
    /// The values above remain as a backwards-compatible fallback for older
    /// project files and X/Y/custom axes.
    #[serde(default)]
    pub axis_line_entity_id: Option<u64>,
    pub angle_deg: f64,
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub operation: ExtrudeOperation,
    #[serde(default)]
    pub target_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditRevolveRequest {
    pub feature_id: FeatureId,
    pub revolve: RevolveRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileRefDto {
    pub sketch_name: String,
    pub profile_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathRefDto {
    pub sketch_name: String,
    pub entity_ids: Vec<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SweepOrientation {
    #[default]
    CorrectedFrenet,
    Frenet,
    Fixed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SweepTransition {
    #[default]
    Transformed,
    RightCorner,
    RoundCorner,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepRequest {
    pub profile: ProfileRefDto,
    pub path_sketch_name: String,
    pub path_entity_ids: Vec<u64>,
    #[serde(default)]
    pub operation: ExtrudeOperation,
    #[serde(default)]
    pub target_body_ids: Vec<BodyId>,
    #[serde(default)]
    pub guide_rail: Option<PathRefDto>,
    #[serde(default)]
    pub orientation: SweepOrientation,
    #[serde(default)]
    pub transition: SweepTransition,
    #[serde(default)]
    pub force_c1: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditSweepRequest {
    pub feature_id: FeatureId,
    pub sweep: SweepRequest,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoftContinuity {
    G0,
    #[default]
    G1,
    G2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoftRequest {
    pub sections: Vec<ProfileRefDto>,
    #[serde(default)]
    pub ruled: bool,
    #[serde(default)]
    pub operation: ExtrudeOperation,
    #[serde(default)]
    pub target_body_ids: Vec<BodyId>,
    #[serde(default)]
    pub continuity: LoftContinuity,
    #[serde(default)]
    pub centerline: Option<PathRefDto>,
    #[serde(default)]
    pub guide_rail: Option<PathRefDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditLoftRequest {
    pub feature_id: FeatureId,
    pub loft: LoftRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RibExtent {
    Distance { depth: f64 },
    ToNext,
    ToFace { face_id: FaceId },
    ThroughAll,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RibRequest {
    pub sketch_name: String,
    pub line_entity_ids: Vec<u64>,
    pub thickness: f64,
    pub depth: f64,
    #[serde(default)]
    pub symmetric: bool,
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub operation: ExtrudeOperation,
    #[serde(default)]
    pub target_body_ids: Vec<BodyId>,
    /// `None` preserves finite-depth projects from the first Rib slice.
    #[serde(default)]
    pub extent: Option<RibExtent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditRibRequest {
    pub feature_id: FeatureId,
    pub rib: RibRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolidFilletRequest {
    pub body_id: BodyId,
    pub edge_ids: Vec<EdgeId>,
    pub radius: f64,
    /// Expand every selected edge through tangent-continuous neighbors.
    #[serde(default)]
    pub tangent_chain: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditSolidFilletRequest {
    pub feature_id: FeatureId,
    pub fillet: SolidFilletRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolidChamferRequest {
    pub body_id: BodyId,
    pub edge_ids: Vec<EdgeId>,
    pub distance: f64,
    #[serde(default)]
    pub tangent_chain: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditSolidChamferRequest {
    pub feature_id: FeatureId,
    pub chamfer: SolidChamferRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HoleExtent {
    Distance { depth: f64 },
    ThroughAll,
}

impl Default for HoleExtent {
    fn default() -> Self {
        Self::Distance { depth: 10.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleStyle {
    Simple,
    Counterbore,
    Countersink,
}

impl Default for HoleStyle {
    fn default() -> Self {
        Self::Simple
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleBottomStyle {
    Flat,
    DrillPoint,
}

impl Default for HoleBottomStyle {
    fn default() -> Self {
        // Legacy project files described cylindrical cutters, so their
        // omitted value must preserve a flat bottom.
        Self::Flat
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleThreadStandard {
    IsoMetric,
    UnifiedInch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleThreadSeries {
    MetricCoarse,
    MetricFine,
    Unc,
    Unf,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleThreadHand {
    #[default]
    Right,
    Left,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleThreadRepresentation {
    #[default]
    Modeled,
    Simplified,
}

/// Manufacturing and geometry data for a screw thread.
///
/// For a threaded hole, `HoleRequest::diameter` remains the actual predrill
/// cylinder diameter while the thread grows to `nominal_diameter`. For an
/// external thread, `nominal_diameter` is the selected cylinder's major
/// diameter and the modeled groove cuts inward from that surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleThreadDto {
    pub standard: HoleThreadStandard,
    pub series: HoleThreadSeries,
    /// Canonical human-readable callout, for example `M6 x 1 - 6H` or
    /// `1/4-20 UNC-2B`.
    pub designation: String,
    /// Internal-thread tolerance class (`6H` for common ISO metric threads,
    /// `2B` for common Unified threads).
    pub class: String,
    /// Basic major diameter in millimetres.
    pub nominal_diameter: f64,
    /// Axial pitch in millimetres for both metric and Unified threads.
    pub pitch: f64,
    /// Original inch-series pitch value retained for an unambiguous callout.
    #[serde(default)]
    pub threads_per_inch: Option<f64>,
    #[serde(default)]
    pub hand: HoleThreadHand,
    /// `None` threads the full cylindrical hole depth. A finite value allows
    /// a blind drilled hole to keep an unthreaded tap/run-out allowance.
    #[serde(default)]
    pub depth: Option<f64>,
    #[serde(default)]
    pub representation: HoleThreadRepresentation,
    /// Optional shop drill label such as `5.0 mm` or `#7`.
    #[serde(default)]
    pub tap_drill_designation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HolePositionDto {
    /// Hole center in the selected face's local (u, v) coordinates.
    pub position: Point2Dto,
    /// Optional associative sketch feature used to recompute this center.
    #[serde(default)]
    pub position_reference: Option<SketchPointRefDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleRequest {
    pub body_id: BodyId,
    pub face_id: FaceId,
    /// Legacy/single-position center. New clients also populate `positions`;
    /// keeping these fields makes existing project archives and MCP clients
    /// load without migration.
    pub position: Point2Dto,
    #[serde(default)]
    pub position_reference: Option<SketchPointRefDto>,
    /// One feature can cut any number of holes with shared parameters.
    #[serde(default)]
    pub positions: Vec<HolePositionDto>,
    pub diameter: f64,
    #[serde(default)]
    pub extent: HoleExtent,
    #[serde(default)]
    pub style: HoleStyle,
    #[serde(default)]
    pub counterbore_diameter: f64,
    #[serde(default)]
    pub counterbore_depth: f64,
    #[serde(default)]
    pub countersink_diameter: f64,
    #[serde(default = "default_countersink_angle")]
    pub countersink_angle_deg: f64,
    #[serde(default)]
    pub bottom_style: HoleBottomStyle,
    #[serde(default = "default_drill_point_angle")]
    pub drill_point_angle_deg: f64,
    #[serde(default)]
    pub thread: Option<HoleThreadDto>,
    #[serde(default)]
    pub flip: bool,
}

fn default_countersink_angle() -> f64 {
    90.0
}

fn default_drill_point_angle() -> f64 {
    118.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditHoleRequest {
    pub feature_id: FeatureId,
    pub hole: HoleRequest,
}

/// Create a male screw thread on an existing cylindrical solid face.
///
/// The exact analytic face is captured when the feature is authored so the
/// feature remains stable after a modeled thread replaces that cylinder with
/// helical topology. `flip` selects which axial end of the cylinder is the
/// thread start when a finite depth is requested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalThreadRequest {
    pub body_id: BodyId,
    pub face_id: FaceId,
    pub thread: HoleThreadDto,
    #[serde(default)]
    pub flip: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetRollbackRequest {
    pub rollback_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteFeatureRequest {
    pub feature_id: FeatureId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReorderFeatureRequest {
    pub feature_id: FeatureId,
    /// Pre-move insertion slot in `0..=feature_count`.
    pub target_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepThreadMetadataDto {
    pub body_id: BodyId,
    pub feature_id: FeatureId,
    pub feature_name: String,
    pub position_count: u32,
    /// True for a male thread on an exterior cylindrical surface. Missing in
    /// older V1 metadata means an internal threaded hole.
    #[serde(default)]
    pub external: bool,
    /// Internal predrill or external shaft diameter in millimetres.
    pub predrill_diameter: f64,
    pub thread: HoleThreadDto,
}

/// One solved component occurrence included in an assembly-aware STEP export.
/// The referenced OCCT body remains the exact part-history B-rep; this rigid
/// placement is applied only to the exchange copy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepOccurrencePlacementDto {
    pub occurrence_id: u64,
    pub component_id: u64,
    pub body_id: BodyId,
    #[serde(default)]
    pub name: String,
    pub translation: [f64; 3],
    /// Unit quaternion in x, y, z, w order.
    pub rotation: [f64; 4],
}

/// STEP export selection. An empty body list means every active body.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StepExportRequest {
    /// Hosts enforce this precondition under the same ownership lock as export.
    #[serde(default)]
    pub expected_model_json: Option<String>,
    #[serde(default)]
    pub body_ids: Vec<BodyId>,
    /// Namespaced manufacturing metadata written into FILE_DESCRIPTION.
    /// Modeled threads additionally travel as ordinary AP242 B-rep geometry.
    #[serde(default)]
    pub thread_metadata: Vec<StepThreadMetadataDto>,
    /// When populated, export exact placed occurrence copies instead of one
    /// unplaced copy per source body. Empty retains part-export behavior.
    #[serde(default)]
    pub occurrences: Vec<StepOccurrencePlacementDto>,
}

/// Shared optimistic precondition for exact and mesh export hosts.
pub fn check_export_model_snapshot(
    expected: Option<&str>,
    current: &str,
) -> Result<(), &'static str> {
    if expected.is_some_and(|expected| expected != current) {
        return Err("The document changed while preparing export. Start the export again.");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    /// Stable source identity retained in parametric history.
    #[serde(default)]
    pub source_face: Option<PlanarFaceSourceDto>,
    /// Body-local OCCT topology label captured before this feature mutates
    /// its target. This ordinal is retained for diagnostics only; replay must
    /// validate `source_face_signature` rather than trusting it.
    #[serde(default)]
    pub source_face_key: Option<String>,
    /// Exact geometric/topological fingerprint captured with the face key.
    /// OCCT face-map ordinals are only an acceleration hint: every replay
    /// validates this signature and searches the body when the ordinal moved.
    #[serde(default)]
    pub source_face_signature: Option<PlanarFaceSignatureDto>,
    /// Creation-time plane cache bootstraps project reload before a fresh
    /// kernel scene is available. The validated signature remains the
    /// modeling truth.
    #[serde(default)]
    pub source_face_basis: Option<PlaneBasis>,
    pub sketch_name: String,
    pub profile_indices: Vec<u32>,
    pub operation: ExtrudeOperation,
    pub extent: ExtrudeExtent,
    pub taper_angle_deg: f64,
    pub flip: bool,
    pub target_body_ids: Vec<BodyId>,
    /// Last resolved target plane for a To Face extent. This small
    /// parametric reference cache lets a project bootstrap its first replay;
    /// it is refreshed from the stable FaceId on every later recompute.
    #[serde(default)]
    pub to_face_basis: Option<PlaneBasis>,
    /// Stable ids reserved for New Body output profiles. They are retained
    /// when the feature is temporarily changed to a boolean operation.
    pub new_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevolveDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    pub sketch_name: String,
    pub profile_indices: Vec<u32>,
    pub axis_origin: Point2Dto,
    pub axis_direction: Point2Dto,
    #[serde(default)]
    pub axis_line_sketch_name: Option<String>,
    #[serde(default)]
    pub axis_line_entity_id: Option<u64>,
    pub angle_deg: f64,
    pub flip: bool,
    #[serde(default)]
    pub operation: ExtrudeOperation,
    #[serde(default)]
    pub target_body_ids: Vec<BodyId>,
    /// Stable ids reserved for each selected profile's New Body result.
    pub new_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    pub profile: ProfileRefDto,
    pub path_sketch_name: String,
    pub path_entity_ids: Vec<u64>,
    pub operation: ExtrudeOperation,
    pub target_body_ids: Vec<BodyId>,
    pub new_body_id: BodyId,
    #[serde(default)]
    pub guide_rail: Option<PathRefDto>,
    #[serde(default)]
    pub orientation: SweepOrientation,
    #[serde(default)]
    pub transition: SweepTransition,
    #[serde(default)]
    pub force_c1: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoftDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    pub sections: Vec<ProfileRefDto>,
    pub ruled: bool,
    pub operation: ExtrudeOperation,
    pub target_body_ids: Vec<BodyId>,
    pub new_body_id: BodyId,
    #[serde(default)]
    pub continuity: LoftContinuity,
    #[serde(default)]
    pub centerline: Option<PathRefDto>,
    #[serde(default)]
    pub guide_rail: Option<PathRefDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RibDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    pub sketch_name: String,
    pub line_entity_ids: Vec<u64>,
    pub thickness: f64,
    pub depth: f64,
    pub symmetric: bool,
    pub flip: bool,
    pub operation: ExtrudeOperation,
    pub target_body_ids: Vec<BodyId>,
    pub new_body_ids: Vec<BodyId>,
    #[serde(default)]
    pub extent: Option<RibExtent>,
    #[serde(default)]
    pub to_face_basis: Option<PlaneBasis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolidFilletDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    pub body_id: BodyId,
    pub edge_ids: Vec<EdgeId>,
    /// Kernel topology labels cached for first replay after opening a project.
    #[serde(default)]
    pub edge_keys: Vec<String>,
    pub radius: f64,
    #[serde(default)]
    pub tangent_chain: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolidChamferDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    pub body_id: BodyId,
    pub edge_ids: Vec<EdgeId>,
    #[serde(default)]
    pub edge_keys: Vec<String>,
    pub distance: f64,
    #[serde(default)]
    pub tangent_chain: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoleDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    pub body_id: BodyId,
    pub face_id: FaceId,
    pub position: Point2Dto,
    #[serde(default)]
    pub position_reference: Option<SketchPointRefDto>,
    #[serde(default)]
    pub positions: Vec<HolePositionDto>,
    pub diameter: f64,
    pub extent: HoleExtent,
    pub style: HoleStyle,
    pub counterbore_diameter: f64,
    pub counterbore_depth: f64,
    pub countersink_diameter: f64,
    pub countersink_angle_deg: f64,
    #[serde(default)]
    pub bottom_style: HoleBottomStyle,
    #[serde(default = "default_drill_point_angle")]
    pub drill_point_angle_deg: f64,
    #[serde(default)]
    pub thread: Option<HoleThreadDto>,
    pub flip: bool,
    /// Cached planar support lets a saved project bootstrap its first replay.
    #[serde(default)]
    pub face_basis: Option<PlaneBasis>,
}

// --- Construction planes -------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DatumPlaneSourceDto {
    Offset {
        reference: PlaneRef,
        distance: f64,
    },
    Midplane {
        first: PlaneRef,
        second: PlaneRef,
    },
    AtAngle {
        reference: PlaneRef,
        body_id: BodyId,
        edge_id: EdgeId,
        angle_deg: f64,
        /// Cached endpoints bootstrap project replay before the first kernel
        /// scene has been reconstructed.
        #[serde(default)]
        axis_points: Option<[Point3Dto; 2]>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatumPlaneRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub source: DatumPlaneSourceDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditDatumPlaneRequest {
    pub feature_id: FeatureId,
    pub plane: DatumPlaneRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatumPlaneDefinitionDto {
    pub feature_id: FeatureId,
    pub name: String,
    pub datum_id: FaceId,
    pub source: DatumPlaneSourceDto,
    pub basis: PlaneBasis,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatumPlaneUpdateDto {
    pub document: DocumentDto,
    pub planes: Vec<DatumPlaneDefinitionDto>,
}

// --- Body-level history features ----------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellRequest {
    pub body_id: BodyId,
    pub face_ids: Vec<FaceId>,
    pub thickness: f64,
    #[serde(default)]
    pub inward: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolidMirrorRequest {
    pub body_ids: Vec<BodyId>,
    pub plane: PlaneRef,
    #[serde(default)]
    pub plane_basis: Option<PlaneBasis>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveCopyBodyRequest {
    pub body_ids: Vec<BodyId>,
    /// Final rigid transform applied to each source body. Rotation is a unit
    /// quaternion in x, y, z, w order and translation is in millimetres.
    pub translation: Point3Dto,
    #[serde(default = "identity_quaternion")]
    pub rotation: [f64; 4],
    /// Pivot used by the rotation, normally the selection bounds center.
    pub pivot: Point3Dto,
    #[serde(default)]
    pub copy: bool,
}

fn identity_quaternion() -> [f64; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RectangularPatternRequest {
    pub body_ids: Vec<BodyId>,
    pub direction: Point3Dto,
    pub spacing: f64,
    pub count: u32,
    #[serde(default)]
    pub second_direction: Option<Point3Dto>,
    #[serde(default)]
    pub second_spacing: f64,
    #[serde(default = "default_pattern_count")]
    pub second_count: u32,
}

fn default_pattern_count() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircularPatternRequest {
    pub body_ids: Vec<BodyId>,
    pub axis_origin: Point3Dto,
    pub axis_direction: Point3Dto,
    pub count: u32,
    #[serde(default = "default_full_angle")]
    pub total_angle_deg: f64,
}

fn default_full_angle() -> f64 {
    360.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombineOperation {
    Join,
    Cut,
    Intersect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CombineRequest {
    pub target_body_id: BodyId,
    pub tool_body_ids: Vec<BodyId>,
    pub operation: CombineOperation,
    #[serde(default)]
    pub keep_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitBodyRequest {
    pub body_id: BodyId,
    pub plane: PlaneRef,
    #[serde(default)]
    pub plane_basis: Option<PlaneBasis>,
}

/// Import one STEP/STP exchange file as a persistent compound body. The
/// source bytes are base64 so project JSON remains compact enough to ZIP and
/// host adapters can carry the request without platform-specific file paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportStepRequest {
    pub file_name: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "request", rename_all = "snake_case")]
pub enum BodyFeatureRequestDto {
    ExternalThread(ExternalThreadRequest),
    Shell(ShellRequest),
    MoveCopy(MoveCopyBodyRequest),
    Mirror(SolidMirrorRequest),
    RectangularPattern(RectangularPatternRequest),
    CircularPattern(CircularPatternRequest),
    Combine(CombineRequest),
    SplitBody(SplitBodyRequest),
    ImportStep(ImportStepRequest),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditBodyFeatureRequest {
    pub feature_id: FeatureId,
    pub feature: BodyFeatureRequestDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BodyFeatureDefinitionDto {
    ExternalThread {
        feature_id: FeatureId,
        name: String,
        body_id: BodyId,
        face_id: FaceId,
        face_key: String,
        cylinder: CylindricalSurfaceDto,
        thread: HoleThreadDto,
        flip: bool,
    },
    Shell {
        feature_id: FeatureId,
        name: String,
        body_id: BodyId,
        face_ids: Vec<FaceId>,
        #[serde(default)]
        face_keys: Vec<String>,
        thickness: f64,
        inward: bool,
    },
    MoveCopy {
        feature_id: FeatureId,
        name: String,
        body_ids: Vec<BodyId>,
        translation: Point3Dto,
        rotation: [f64; 4],
        pivot: Point3Dto,
        copy: bool,
        result_body_ids: Vec<BodyId>,
    },
    Mirror {
        feature_id: FeatureId,
        name: String,
        body_ids: Vec<BodyId>,
        plane: PlaneRef,
        plane_basis: PlaneBasis,
        new_body_ids: Vec<BodyId>,
    },
    RectangularPattern {
        feature_id: FeatureId,
        name: String,
        body_ids: Vec<BodyId>,
        direction: Point3Dto,
        spacing: f64,
        count: u32,
        second_direction: Option<Point3Dto>,
        second_spacing: f64,
        second_count: u32,
        new_body_ids: Vec<BodyId>,
    },
    CircularPattern {
        feature_id: FeatureId,
        name: String,
        body_ids: Vec<BodyId>,
        axis_origin: Point3Dto,
        axis_direction: Point3Dto,
        count: u32,
        total_angle_deg: f64,
        new_body_ids: Vec<BodyId>,
    },
    Combine {
        feature_id: FeatureId,
        name: String,
        target_body_id: BodyId,
        tool_body_ids: Vec<BodyId>,
        operation: CombineOperation,
        keep_tools: bool,
    },
    SplitBody {
        feature_id: FeatureId,
        name: String,
        body_id: BodyId,
        plane: PlaneRef,
        plane_basis: PlaneBasis,
        new_body_id: BodyId,
    },
    ImportStep {
        feature_id: FeatureId,
        name: String,
        file_name: String,
        data_base64: String,
        body_id: BodyId,
    },
}

impl BodyFeatureDefinitionDto {
    pub fn feature_id(&self) -> FeatureId {
        match self {
            Self::ExternalThread { feature_id, .. }
            | Self::Shell { feature_id, .. }
            | Self::MoveCopy { feature_id, .. }
            | Self::Mirror { feature_id, .. }
            | Self::RectangularPattern { feature_id, .. }
            | Self::CircularPattern { feature_id, .. }
            | Self::Combine { feature_id, .. }
            | Self::SplitBody { feature_id, .. }
            | Self::ImportStep { feature_id, .. } => *feature_id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::ExternalThread { name, .. }
            | Self::Shell { name, .. }
            | Self::MoveCopy { name, .. }
            | Self::Mirror { name, .. }
            | Self::RectangularPattern { name, .. }
            | Self::CircularPattern { name, .. }
            | Self::Combine { name, .. }
            | Self::SplitBody { name, .. }
            | Self::ImportStep { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelProfileDto {
    pub profile_index: u32,
    pub points: Vec<Point3Dto>,
    #[serde(default)]
    pub curves: Vec<KernelCurveDto>,
    /// Immediate odd-depth child loops. Kernels add these as inner wires.
    #[serde(default)]
    pub holes: Vec<KernelProfileDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KernelCurveDto {
    Line {
        entity_id: u64,
        start: Point3Dto,
        end: Point3Dto,
    },
    Arc {
        entity_id: u64,
        start: Point3Dto,
        mid: Point3Dto,
        end: Point3Dto,
    },
    Circle {
        entity_id: u64,
        center: Point3Dto,
        /// A point on the circle defining its radius and local X direction.
        axis_point: Point3Dto,
        /// Unit plane normal encoded as xyz components.
        normal: Point3Dto,
    },
    Polyline {
        entity_id: u64,
        points: Vec<Point3Dto>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelExtrudeJobDto {
    pub feature_id: FeatureId,
    pub operation: ExtrudeOperation,
    /// Exact B-rep face source. When set, `profiles` is empty and the OCCT
    /// adapter resolves the unique face matching the validated signature.
    #[serde(default)]
    pub source_face: Option<KernelPlanarFaceSourceDto>,
    pub profiles: Vec<KernelProfileDto>,
    pub normal: Point3Dto,
    pub start_offset: f64,
    pub end_offset: f64,
    pub taper_angle_deg: f64,
    pub target_body_ids: Vec<BodyId>,
    pub result_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelPlanarFaceSourceDto {
    pub body_id: BodyId,
    pub face_id: FaceId,
    pub face_key: String,
    pub signature: PlanarFaceSignatureDto,
}

/// Cross-host fingerprint for an exact planar B-rep face. Values come from
/// OCCT properties rather than display tessellation, so native and browser
/// replay can validate the same project. This is deliberately conservative:
/// an ambiguous or changed match is a broken reference, never an ordinal
/// retarget to a different face.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlanarFaceSignatureDto {
    pub centroid: Point3Dto,
    pub normal: Point3Dto,
    pub area: f64,
    pub perimeter: f64,
    pub wire_count: u32,
    pub edge_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelRevolveJobDto {
    pub feature_id: FeatureId,
    pub operation: ExtrudeOperation,
    pub profiles: Vec<KernelProfileDto>,
    pub axis_origin: Point3Dto,
    pub axis_direction: Point3Dto,
    pub angle_rad: f64,
    pub target_body_ids: Vec<BodyId>,
    pub result_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelSweepJobDto {
    pub feature_id: FeatureId,
    pub operation: ExtrudeOperation,
    pub profile: KernelProfileDto,
    pub path: Vec<KernelCurveDto>,
    #[serde(default)]
    pub guide_rail: Vec<KernelCurveDto>,
    pub orientation: SweepOrientation,
    pub transition: SweepTransition,
    pub force_c1: bool,
    pub target_body_ids: Vec<BodyId>,
    pub result_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelLoftJobDto {
    pub feature_id: FeatureId,
    pub operation: ExtrudeOperation,
    pub sections: Vec<KernelProfileDto>,
    pub ruled: bool,
    pub continuity: LoftContinuity,
    #[serde(default)]
    pub centerline: Vec<KernelCurveDto>,
    #[serde(default)]
    pub guide_rail: Vec<KernelCurveDto>,
    pub target_body_ids: Vec<BodyId>,
    pub result_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelRibJobDto {
    pub feature_id: FeatureId,
    pub operation: ExtrudeOperation,
    pub profiles: Vec<KernelProfileDto>,
    pub normal: Point3Dto,
    pub start_offset: f64,
    pub end_offset: f64,
    pub target_body_ids: Vec<BodyId>,
    pub result_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KernelTransformDto {
    Mirror {
        origin: Point3Dto,
        normal: Point3Dto,
    },
    Translate {
        vector: Point3Dto,
    },
    Rotate {
        origin: Point3Dto,
        axis: Point3Dto,
        angle_rad: f64,
    },
    Rigid {
        translation: Point3Dto,
        rotation: [f64; 4],
        pivot: Point3Dto,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelTransformJobDto {
    pub feature_id: FeatureId,
    pub source_body_ids: Vec<BodyId>,
    pub transforms: Vec<KernelTransformDto>,
    /// Transform-major, then source-body-major.
    pub result_body_ids: Vec<BodyId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelShellJobDto {
    pub feature_id: FeatureId,
    pub target_body_id: BodyId,
    pub face_keys: Vec<String>,
    pub thickness: f64,
    pub inward: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelCombineJobDto {
    pub feature_id: FeatureId,
    pub target_body_id: BodyId,
    pub tool_body_ids: Vec<BodyId>,
    pub operation: CombineOperation,
    pub keep_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelSplitBodyJobDto {
    pub feature_id: FeatureId,
    pub target_body_id: BodyId,
    pub plane_origin: Point3Dto,
    pub plane_normal: Point3Dto,
    pub new_body_id: BodyId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelImportStepJobDto {
    pub feature_id: FeatureId,
    pub result_body_id: BodyId,
    pub data_base64: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelFilletJobDto {
    pub feature_id: FeatureId,
    pub target_body_id: BodyId,
    pub edge_keys: Vec<String>,
    pub radius: f64,
    pub tangent_chain: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelChamferJobDto {
    pub feature_id: FeatureId,
    pub target_body_id: BodyId,
    pub edge_keys: Vec<String>,
    pub distance: f64,
    pub tangent_chain: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelHoleJobDto {
    pub feature_id: FeatureId,
    pub target_body_id: BodyId,
    pub center: Point3Dto,
    /// Direction from the support face into the body.
    pub direction: Point3Dto,
    pub diameter: f64,
    pub extent: HoleExtent,
    pub style: HoleStyle,
    pub counterbore_diameter: f64,
    pub counterbore_depth: f64,
    pub countersink_diameter: f64,
    pub countersink_angle_deg: f64,
    pub bottom_style: HoleBottomStyle,
    pub drill_point_angle_deg: f64,
    pub thread: Option<HoleThreadDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelExternalThreadJobDto {
    pub feature_id: FeatureId,
    pub target_body_id: BodyId,
    /// Stable OCCT topology lookup captured from the selected face.
    pub face_key: String,
    /// Exact analytic fallback used for validation and browser replay.
    pub cylinder: CylindricalSurfaceDto,
    pub thread: HoleThreadDto,
    pub flip: bool,
}

/// Ordered full-replay kernel operation. The tagged representation keeps the
/// plan extensible for Sweep/Loft/Rib without weakening individual DTOs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "job", rename_all = "snake_case")]
pub enum KernelJobDto {
    Extrude(KernelExtrudeJobDto),
    Revolve(KernelRevolveJobDto),
    Sweep(KernelSweepJobDto),
    Loft(KernelLoftJobDto),
    Rib(KernelRibJobDto),
    Fillet(KernelFilletJobDto),
    Chamfer(KernelChamferJobDto),
    Hole(KernelHoleJobDto),
    ExternalThread(KernelExternalThreadJobDto),
    Shell(KernelShellJobDto),
    Transform(KernelTransformJobDto),
    Combine(KernelCombineJobDto),
    SplitBody(KernelSplitBodyJobDto),
    ImportStep(KernelImportStepJobDto),
}

impl KernelJobDto {
    pub fn feature_id(&self) -> FeatureId {
        match self {
            Self::Extrude(job) => job.feature_id,
            Self::Revolve(job) => job.feature_id,
            Self::Sweep(job) => job.feature_id,
            Self::Loft(job) => job.feature_id,
            Self::Rib(job) => job.feature_id,
            Self::Fillet(job) => job.feature_id,
            Self::Chamfer(job) => job.feature_id,
            Self::Hole(job) => job.feature_id,
            Self::ExternalThread(job) => job.feature_id,
            Self::Shell(job) => job.feature_id,
            Self::Transform(job) => job.feature_id,
            Self::Combine(job) => job.feature_id,
            Self::SplitBody(job) => job.feature_id,
            Self::ImportStep(job) => job.feature_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecomputePlanDto {
    pub transaction_id: u64,
    pub jobs: Vec<KernelJobDto>,
    /// Planning failures tied to individual history features. These are
    /// carried through the kernel transaction so independent later features
    /// can still recompute after a dependency is deleted.
    #[serde(default)]
    pub errors: Vec<KernelFeatureErrorDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelFaceDto {
    /// Backend-local topology label used as a fast lookup hint.
    pub key: String,
    pub first_index: u32,
    pub index_count: u32,
    pub plane: Option<PlaneBasis>,
    #[serde(default)]
    pub signature: Option<PlanarFaceSignatureDto>,
    /// Exact analytic cylinder metadata supplied by OCCT. This is separate
    /// from tessellation so picking and joint frames do not infer axes from
    /// triangles.
    #[serde(default)]
    pub cylinder: Option<CylindricalSurfaceDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CylindricalSurfaceDto {
    pub origin: Point3Dto,
    pub axis: Point3Dto,
    pub reference: Point3Dto,
    pub radius: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelEdgeDto {
    pub key: String,
    pub points: Vec<Point3Dto>,
    /// Exact analytic circle metadata supplied by OCCT. Closed circular edges
    /// are first-class joint connector references, independent of tessellation.
    #[serde(default)]
    pub circle: Option<CircularCurveDto>,
    /// True when the edge is a real break between two faces and can be used
    /// by edge-refinement tools such as fillet and chamfer.
    #[serde(default = "default_true")]
    pub refinable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CircularCurveDto {
    pub center: Point3Dto,
    pub normal: Point3Dto,
    pub reference: Point3Dto,
    pub radius: f64,
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelBodyDto {
    pub body_id: BodyId,
    /// Exact native connectivity, without dimensional coordinates.
    #[serde(default)]
    pub topology_signature: String,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub faces: Vec<KernelFaceDto>,
    pub edges: Vec<KernelEdgeDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelFeatureErrorDto {
    pub feature_id: FeatureId,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KernelSceneDto {
    pub bodies: Vec<KernelBodyDto>,
    #[serde(default)]
    pub errors: Vec<KernelFeatureErrorDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitKernelRequest {
    pub transaction_id: u64,
    pub scene: KernelSceneDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshDto {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceDto {
    pub id: FaceId,
    pub key: String,
    pub first_index: u32,
    pub index_count: u32,
    pub plane: Option<PlaneBasis>,
    #[serde(default)]
    pub signature: Option<PlanarFaceSignatureDto>,
    #[serde(default)]
    pub cylinder: Option<CylindricalSurfaceDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeDto {
    pub id: EdgeId,
    pub key: String,
    pub points: Vec<Point3Dto>,
    #[serde(default)]
    pub circle: Option<CircularCurveDto>,
    #[serde(default = "default_true")]
    pub refinable: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyDto {
    pub id: BodyId,
    #[serde(default)]
    pub topology_signature: String,
    pub name: String,
    pub feature_id: FeatureId,
    pub mesh: MeshDto,
    pub faces: Vec<FaceDto>,
    pub edges: Vec<EdgeDto>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SolidSceneDto {
    pub bodies: Vec<BodyDto>,
    pub errors: Vec<KernelFeatureErrorDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolidUpdateDto {
    pub document: DocumentDto,
    pub scene: SolidSceneDto,
}
