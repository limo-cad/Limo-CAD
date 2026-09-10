//! Serializable DTOs for the sketch-session API, exchanged as JSON over both
//! hosts. The frontend TypeScript types in `src/engine/types.ts` mirror these
//! 1:1.

use serde::{Deserialize, Serialize};

use nbcad_core::{DimensionStyle, DocumentDto, EdgeId};

use crate::constraint::{Constraint, ConstraintId};
use crate::entity::EntityId;
use crate::geometry::Vec2;
use crate::params::ParamId;
use crate::plane::{PlaneBasis, PlaneRef};
use crate::sketch::DimensionMode;

/// Project-owned visibility choices for model objects shown in the Browser.
///
/// Browser row ids are reconstructed UI details, so persistence uses stable
/// model identities (body/datum ids) and the unique saved sketch name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectVisibilityDto {
    #[serde(default)]
    pub hidden_body_ids: Vec<u64>,
    #[serde(default)]
    pub hidden_datum_plane_ids: Vec<u64>,
    #[serde(default)]
    pub hidden_sketch_names: Vec<String>,
}

/// Show/hide retained construction references using existing saved visibility.
/// With no selectors, affect all retained references. If either selector is
/// supplied, affect only the explicitly selected sets (an empty set is a no-op).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstructionVisibilityRequest {
    pub visible: bool,
    pub sketch_names: Option<Vec<String>>,
    pub datum_plane_ids: Option<Vec<u64>>,
}

/// One entity in a sketch snapshot. Lines carry both their endpoint point
/// ids (structural coincident) and the resolved endpoint coordinates so the
/// frontend can render without resolving references itself.
/// `fully_defined` comes from the solver's per-entity free-variable
/// analysis and drives constraint-state coloring (blue vs. defined).
/// NOT Copy: the spline variant owns its point lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityDto {
    Point {
        id: EntityId,
        position: Vec2,
        fully_defined: bool,
    },
    Line {
        id: EntityId,
        start_id: EntityId,
        end_id: EntityId,
        start: Vec2,
        end: Vec2,
        fully_defined: bool,
        /// Retained for parametric editability after fillet/chamfer trims
        /// consume the complete visible span.
        #[serde(default)]
        consumed: bool,
    },
    Arc {
        id: EntityId,
        center: Vec2,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        fully_defined: bool,
    },
    Circle {
        id: EntityId,
        center: Vec2,
        radius: f64,
        fully_defined: bool,
    },
    /// Fit-point spline: fit points plus the engine-tessellated polyline
    /// (centripetal Catmull-Rom), so the frontend renders exactly what the
    /// engine computed — single source of truth for the curve shape.
    Spline {
        id: EntityId,
        points: Vec<Vec2>,
        tessellation: Vec<Vec2>,
        fully_defined: bool,
    },
}

impl EntityDto {
    pub fn id(&self) -> EntityId {
        match *self {
            EntityDto::Point { id, .. }
            | EntityDto::Line { id, .. }
            | EntityDto::Arc { id, .. }
            | EntityDto::Circle { id, .. }
            | EntityDto::Spline { id, .. } => id,
        }
    }
}

/// One constraint in a sketch snapshot (flattened: `{"id": 3, "type":
/// "horizontal", "entity": 5}`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConstraintDto {
    pub id: ConstraintId,
    #[serde(flatten)]
    pub constraint: Constraint,
}

/// Human-readable entity reference for an over-constraint rejection report,
/// e.g. `{"id": 5, "label": "Line5"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityDesc {
    pub id: EntityId,
    pub label: String,
}

/// Human-readable constraint description for the rejection report, e.g.
/// "Perpendicular between Line3 and Line5".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstraintDesc {
    pub id: ConstraintId,
    pub kind: String,
    pub entities: Vec<EntityDesc>,
}

/// Degrees-of-freedom result from the sketch solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DofDto {
    pub value: i32,
    pub fully_defined: bool,
}

/// Full snapshot of the active sketch, sent after every mutation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchDto {
    pub name: String,
    pub plane: PlaneRef,
    pub basis: PlaneBasis,
    pub entities: Vec<EntityDto>,
    pub constraints: Vec<ConstraintDto>,
    /// Midpoints of coplanar support-face edges that are available as
    /// external snap references while editing a face-hosted sketch.
    #[serde(default)]
    pub reference_midpoints: Vec<ReferenceMidpointDto>,
    /// Driving dimensions with presentation data (D9).
    pub dimensions: Vec<DimensionDto>,
    pub dimension_style: DimensionStyle,
    pub dof: DofDto,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReferenceMidpointDto {
    pub edge_id: EdgeId,
    pub position: Vec2,
}

/// Placement of sketch coordinate zero when the support is a planar body
/// face. This is a creation-time choice; the resolved basis is persisted in
/// the project so later loads do not depend on tessellation details.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceSketchOrigin {
    /// Backwards-compatible placement used by legacy bare-PlaneRef calls.
    #[default]
    SupportOrigin,
    FaceCenter,
    GlobalOriginProjection,
}

/// Extended Create Sketch payload. The host also accepts a bare `PlaneRef`
/// for backwards compatibility with existing MCP clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeginSketchRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub plane: PlaneRef,
    #[serde(default)]
    pub face_origin: FaceSketchOrigin,
}

/// One dimension in a snapshot. Driving dimensions carry a parameter;
/// reference dimensions carry only their live measured value and annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionDto {
    pub constraint_id: ConstraintId,
    pub mode: DimensionMode,
    /// "distance" | "radius" | "diameter" | "angle"
    pub kind: String,
    pub entities: Vec<EntityId>,
    pub param_id: Option<ParamId>,
    pub param_name: Option<String>,
    pub param_expression: Option<String>,
    pub value: f64,
    /// Formatted annotation text (mm/deg, 2 decimals; Ø/R prefixes).
    pub text: String,
    pub text_pos: Vec2,
}

/// What the cursor snapped to, in priority order (point > origin > exact
/// curve crossing > line midpoint > grid > raw). `Point`/`Origin` snaps
/// imply a coincident inference; `Intersection` is recomputed from its two
/// authoritative carriers and remains constrained to both after commit;
/// `Midpoint` and `ReferenceMidpoint` imply auto-created persistent midpoint
/// constraints on commit (M1d, D4.1 parity) and are suppressed while Ctrl is
/// held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapTarget {
    None,
    Grid,
    Origin,
    Point {
        entity: EntityId,
    },
    /// Cursor snapped to a line's midpoint; `entity` is the host line.
    Midpoint {
        entity: EntityId,
    },
    /// Cursor snapped to the midpoint of a coplanar support-face edge.
    /// Commit binds the point to this stable edge id so future dimension
    /// edits and support-geometry refreshes preserve the exact midpoint.
    ReferenceMidpoint {
        edge: EdgeId,
    },
    /// Exact intersection with a sketch curve acquired by the viewport.
    /// The endpoint remains a distinct point and commit adds its persistent
    /// point-on-curve relation.
    Curve {
        entity: EntityId,
    },
    /// Exact finite-curve crossing acquired in screen space. Coordinates are
    /// never trusted across the UI/engine boundary: the engine recomputes the
    /// analytic intersection from both stable entity ids.
    Intersection {
        first: EntityId,
        second: EntityId,
    },
}

/// Constraints the engine would create (or, for coincident, structurally
/// apply) for a segment, reported during preview for glyph rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Inference {
    Horizontal,
    Vertical,
    Perpendicular,
    Coincident,
}

/// Axis followed by temporary point tracking while a line endpoint is free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackingAxis {
    Horizontal,
    Vertical,
}

/// A point acquired by the viewport for horizontal/vertical object-snap
/// tracking. Acquisition is screen-space; the engine owns the exact math and
/// creates the persistent relation when the line is committed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LineTrackingRequest {
    pub point: EntityId,
    pub axis: TrackingAxis,
}

/// A horizontal/vertical endpoint intent intersected with an existing
/// line, circle, or arc. Screen-space acquisition belongs to the viewport;
/// the engine recomputes the exact intersection and owns the relation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LineIntersectionRequest {
    pub curve: EntityId,
    pub axis: TrackingAxis,
}

/// Exact crossing between two finite sketch curves. This is separate from
/// [`LineIntersectionRequest`], which means a new line's inferred H/V axis
/// intersecting one carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurveCrossingRequest {
    pub first: EntityId,
    pub second: EntityId,
}

/// Exact guide returned with a line preview for native/browser rendering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackingGuideDto {
    pub point: EntityId,
    pub axis: TrackingAxis,
    pub source: Vec2,
    pub snapped_to: Vec2,
}

// --- Requests ---

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SegmentRequest {
    pub from: Vec2,
    pub to_raw: Vec2,
    /// Holding Ctrl temporarily disables inference.
    #[serde(default)]
    pub ctrl_held: bool,
}

/// Drag phase for `move_point`. A rubber-band drag is one undoable command:
/// `begin` captures the pre-drag state, `update`s mutate, `end` commits.
/// `single` = begin+update+end in one call (e.g. scripted moves).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragPhase {
    Begin,
    Update,
    End,
    #[default]
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MovePointRequest {
    pub point_id: EntityId,
    pub to_raw: Vec2,
    #[serde(default)]
    pub ctrl_held: bool,
    #[serde(default)]
    pub phase: DragPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DeleteEntityRequest {
    pub entity_id: EntityId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteEntitiesRequest {
    pub entity_ids: Vec<EntityId>,
}

/// Apply several panel constraints as one transaction and one undo record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintBatchRequest {
    pub constraints: Vec<Constraint>,
}

/// Fix/Unfix several selected entities as one transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToggleFixBatchRequest {
    pub entity_ids: Vec<EntityId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SetGridSnapRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SetGridStepRequest {
    pub step_mm: f64,
}

/// Dynamic-input locked segment request (length in mm, angle in degrees
/// from the plane's +u axis, CCW positive). The `*_text` fields carry the
/// raw typed text (number or formula) — present ⇒ auto-create the driving
/// dimension (D9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedSegmentRequest {
    pub from: Vec2,
    pub to_hint: Vec2,
    /// Optional exact topological identity for the chain start. The viewport
    /// supplies ids; the engine recomputes the crossing coordinate.
    #[serde(default)]
    pub from_crossing: Option<CurveCrossingRequest>,
    /// Optional exact topological identity for the segment endpoint.
    #[serde(default)]
    pub to_crossing: Option<CurveCrossingRequest>,
    #[serde(default)]
    pub length_mm: Option<f64>,
    #[serde(default)]
    pub angle_deg: Option<f64>,
    #[serde(default)]
    pub length_text: Option<String>,
    #[serde(default)]
    pub angle_text: Option<String>,
    #[serde(default)]
    pub ctrl_held: bool,
    #[serde(default)]
    pub tracking: Option<LineTrackingRequest>,
    #[serde(default)]
    pub intersection: Option<LineIntersectionRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RectangleMode {
    TwoPoint,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircleMode {
    CenterDiameter,
    TwoPoint,
}

/// Slot creation mode (M1 follow-up).
/// CenterToCenter: p1/p2 are the two end-cap arc centers. Overall: p1/p2 are
/// the slot's overall endpoints (centers inset by the radius). CenterPoint:
/// p1 is the slot center, p2 one end-cap center (the other mirrors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotMode {
    CenterToCenter,
    Overall,
    CenterPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotRequest {
    pub mode: SlotMode,
    pub p1: Vec2,
    pub p2: Vec2,
    /// Third-click point: drives the width when no typed/locked width exists
    /// (twice the perpendicular distance to the p1→p2 axis).
    pub cursor: Vec2,
    #[serde(default)]
    pub width_mm: Option<f64>,
    #[serde(default)]
    pub width_text: Option<String>,
}

/// Fit-point spline creation (M1 follow-up): ordered fit points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplineRequest {
    /// Fit points in pick order (≥ 2 after consecutive-duplicate cleanup).
    pub points: Vec<Vec2>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RectangleRequest {
    pub mode: RectangleMode,
    pub p1: Vec2,
    pub p2: Vec2,
    #[serde(default)]
    pub ctrl_held: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedRectangleRequest {
    pub mode: RectangleMode,
    pub anchor: Vec2,
    #[serde(default)]
    pub width_mm: Option<f64>,
    #[serde(default)]
    pub height_mm: Option<f64>,
    #[serde(default)]
    pub width_text: Option<String>,
    #[serde(default)]
    pub height_text: Option<String>,
    pub corner_hint: Vec2,
    #[serde(default)]
    pub ctrl_held: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CircleRequest {
    pub mode: CircleMode,
    pub p1: Vec2,
    pub p2: Vec2,
    #[serde(default)]
    pub ctrl_held: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedCircleRequest {
    pub mode: CircleMode,
    pub anchor: Vec2,
    #[serde(default)]
    pub diameter_mm: Option<f64>,
    #[serde(default)]
    pub diameter_text: Option<String>,
    pub edge_hint: Vec2,
    #[serde(default)]
    pub ctrl_held: bool,
}

// --- Dimension ops (D9) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionRequest {
    /// Entity combinations: [line] length, [p1, p2],
    /// [point, line], [line, line] (distance or angle by parallelism),
    /// [circle] diameter, [arc] radius.
    pub entities: Vec<EntityId>,
    pub text_pos: Vec2,
    /// Typed formula/value for the driving parameter; None = measure the
    /// current geometry (default behavior).
    #[serde(default)]
    pub value_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditDimensionRequest {
    pub constraint_id: ConstraintId,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetDimensionModeRequest {
    pub constraint_id: ConstraintId,
    pub mode: DimensionMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveDimensionRequest {
    pub constraint_id: ConstraintId,
    pub text_pos: Vec2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteDimensionRequest {
    pub constraint_id: ConstraintId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteConstraintRequest {
    pub constraint_id: ConstraintId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetDimensionStyleRequest {
    pub style: DimensionStyle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalExpressionRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalExpressionResult {
    pub value: f64,
}

// --- Modify tools (M1c-ii) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilletRequest {
    pub l1: EntityId,
    pub l2: EntityId,
    pub radius_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilletPreviewDto {
    pub center: Vec2,
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub ccw: bool,
    pub tangent_on_l1: Vec2,
    pub tangent_on_l2: Vec2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamferRequest {
    pub l1: EntityId,
    pub l2: EntityId,
    pub distance_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffsetRequest {
    pub entity: EntityId,
    pub distance_text: String,
    pub cursor: Vec2,
}

/// A curve shape for previews (offset result, trim kept/removed pieces).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviewCurve {
    Line {
        a: Vec2,
        b: Vec2,
    },
    Arc {
        center: Vec2,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    Circle {
        center: Vec2,
        radius: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffsetPreviewDto {
    pub curve: PreviewCurve,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrimRequest {
    pub entity: EntityId,
    pub click: Vec2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrimPreviewDto {
    /// Every surviving connected piece. Middle trims of a line or open arc
    /// legitimately produce two pieces.
    pub kept: Vec<PreviewCurve>,
    pub removed: PreviewCurve,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtendRequest {
    pub entity: EntityId,
    pub click: Vec2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakRequest {
    pub entity: EntityId,
    pub at: Vec2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirrorRequest {
    pub entity_ids: Vec<EntityId>,
    pub axis_line: EntityId,
}

/// Rectangular sketch pattern. Counts include the selected source geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RectangularPatternRequest {
    pub entity_ids: Vec<EntityId>,
    pub direction: Vec2,
    pub spacing: f64,
    pub count: u32,
    #[serde(default)]
    pub second_direction: Option<Vec2>,
    #[serde(default)]
    pub second_spacing: f64,
    #[serde(default = "default_pattern_count")]
    pub second_count: u32,
}

/// Circular sketch pattern. Count includes the selected source geometry.
/// A full 360-degree pattern avoids duplicating the source occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircularPatternRequest {
    pub entity_ids: Vec<EntityId>,
    pub center: Vec2,
    pub count: u32,
    pub total_angle_deg: f64,
}

fn default_pattern_count() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveCopyRequest {
    pub entity_ids: Vec<EntityId>,
    pub dx: f64,
    pub dy: f64,
    pub copy: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScaleRequest {
    pub entity_ids: Vec<EntityId>,
    pub origin: Vec2,
    pub factor_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolygonRequest {
    pub center: Vec2,
    pub edge_count: u32,
    pub radius_text: String,
    pub rotation_deg: f64,
    /// "inscribed" | "circumscribed"
    pub mode: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Arc3PointRequest {
    pub p1: Vec2,
    pub p2: Vec2,
    pub p3: Vec2,
    #[serde(default)]
    pub ctrl_held: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArcCenterRequest {
    pub center: Vec2,
    pub start: Vec2,
    pub sweep: Vec2,
    #[serde(default)]
    pub ctrl_held: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MidpointLineRequest {
    pub mid_raw: Vec2,
    pub end_raw: Vec2,
    #[serde(default)]
    pub ctrl_held: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointRequest {
    pub position: Vec2,
    /// Optional curve acquired by the Point tool. When present, point
    /// creation and its point-on-curve relation are one atomic command.
    #[serde(default)]
    pub coincident_with: Option<EntityId>,
    /// Temporarily suppress inferred point/origin/carrier relations.
    #[serde(default)]
    pub ctrl_held: bool,
}

// --- Results ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewDto {
    /// Cursor position after snapping and H/V inference projection.
    pub snapped_to: Vec2,
    pub snap: SnapTarget,
    /// Constraints that WOULD be created by `add_line` with the same input.
    pub inferences: Vec<Inference>,
    /// Temporary horizontal/vertical alignment to another sketch point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking: Option<TrackingGuideDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddLineResult {
    pub entity_id: EntityId,
    pub start_point_id: EntityId,
    pub end_point_id: EntityId,
    /// Constraints actually created (coincident is structural — it merges
    /// point entities and produces no constraint record).
    pub created_constraints: Vec<ConstraintDto>,
    pub sketch: SketchDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MovePointResult {
    pub sketch: SketchDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteEntityResult {
    /// All entities removed (cascade: deleting a point deletes its lines).
    pub removed: Vec<EntityId>,
    pub sketch: SketchDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndoResult {
    pub sketch: SketchDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddConstraintResult {
    pub constraint_id: ConstraintId,
    pub sketch: SketchDto,
}

/// Generic result of the non-line tool ops (created entity ids + snapshot).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub entities: Vec<EntityId>,
    pub sketch: SketchDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndSketchResult {
    pub document: DocumentDto,
}

/// Uniform result envelope for the JSON host boundary: every host function
/// returns either `{"ok": true, "value": ...}` or `{"ok": false, "error":
/// "..."}`. Both hosts (Tauri commands, wasm-bindgen exports) emit exactly
/// this shape so the frontend adapters are interchangeable.
pub fn ok_json<T: Serialize>(value: T) -> String {
    serde_json::json!({ "ok": true, "value": value }).to_string()
}

pub fn err_json(message: impl Into<String>) -> String {
    serde_json::json!({ "ok": false, "error": message.into() }).to_string()
}
