//! Sketch session: live editing state of one sketch on one plane.
//!
//! Provides snap and auto-constraint inference, the Newton-based constraint
//! solver ([`crate::solver`]) running after every mutation, over-constraint
//! rejection with an explicit conflict report (D4.2), rubber-band dragging
//! via solver pinning, locked dynamic-input endpoint math, and the core
//! tool ops (line, midpoint line, point, rectangle, circle, arc). Undo/redo
//! stays snapshot-based, so every mutation (including solver motion and
//! cascades) restores exactly.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use limo_cad_core::{DimensionStyle, EdgeId};

use crate::constraint::{Constraint, ConstraintId};
use crate::dto::{
    AddConstraintResult, AddLineResult, CircleMode, ConstraintDesc, ConstraintDto,
    CurveCrossingRequest, DeleteEntityResult, DofDto, DragPhase, EntityDesc, EntityDto, Inference,
    LineIntersectionRequest, LineTrackingRequest, LockedCircleRequest, LockedRectangleRequest,
    LockedSegmentRequest, MovePointRequest, MovePointResult, PreviewDto, ProjectedEdgeDto,
    RectangleMode, ReferenceMidpointDto, SketchDto, SlotMode, SlotRequest, SnapTarget,
    SplineRequest, ToolResult, TrackingAxis, TrackingGuideDto, UndoResult,
};
use crate::entity::{Entity, EntityId};
use crate::geometry::Vec2;
use crate::plane::{PlaneBasis, PlaneRef};
use crate::project::ProjectSketchV2;
use crate::sketch::{Sketch, SketchSnapshot};
use crate::solver::{self, Analysis};

mod creation;
mod dims;
mod mods;

/// Snap distance tolerance in sketch mm (screen-relative scaling is a
/// UI concern; the engine works on a fixed modeling tolerance).
pub const SNAP_TOLERANCE_MM: f64 = 2.0;
/// Default grid step in mm; the sketch grid snaps to its intersections when
/// grid snap is on.
pub const GRID_STEP_MM: f64 = 10.0;
/// Grid intersections are magnetic only inside this fraction of one visible
/// minor-grid step. The UI uses the equivalent screen-space radius;
/// this engine-side guard keeps direct/native callers from rounding every
/// free coordinate merely because the grid is enabled.
pub const GRID_CAPTURE_FRACTION: f64 = 0.25;
/// Finest supported modeling grid: one micrometer in the document's mm
/// coordinate system.
pub const MIN_GRID_STEP_MM: f64 = 0.001;
/// Guard against nonsensical or overflowing host input while still allowing
/// very large civil/architectural sketches.
pub const MAX_GRID_STEP_MM: f64 = 1_000_000.0;
/// H/V inference cone half-angle in degrees.
///
/// A narrow cone keeps H/V helpful without silently flattening deliberate
/// shallow diagonals. Ctrl remains the explicit temporary inference override.
pub const INFERENCE_ANGLE_TOL_DEG: f64 = 3.0;
/// Segments shorter than this are rejected as degenerate.
pub const MIN_LINE_LENGTH_MM: f64 = 1e-6;
/// Distance below which two points are considered the same location.
const MERGE_EPS: f64 = 1e-6;
/// Angular travel below which a center-arc sweep counts as "the pointer never
/// moved": the picks sit on one ray and there is no arc to build.
pub const MIN_ARC_TRAVEL_RAD: f64 = 1e-6;
/// Residual above which a fresh constraint counts as inconsistent (D4.2).
const INCONSISTENT_EPS: f64 = 1e-6;

/// Compact number formatting for literal auto-dim parameters (50 not 50.0).
fn format_number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.4}")
    }
}

/// Errors of the sketch-session API. Serialized at the host boundary; the
/// OverConstrained and RedundantConstraint carry structured relation data
/// (D4.2), while ConstraintSolveFailed deliberately does not invent a culprit
/// when the numerical solver cannot establish one.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionError {
    /// `begin_sketch` while another sketch is being edited.
    SketchAlreadyActive,
    /// A drawing op was called without an active sketch session.
    NoActiveSketch,
    /// `edit_sketch` named a sketch that is not in the finished list.
    SketchNotFound(String),
    /// The plane reference cannot be resolved yet (reserved M2/M3 kinds).
    UnsupportedPlane,
    /// A persistent body/face reference no longer resolves after recompute.
    BrokenReference(String),
    /// Solid feature planning/commit error surfaced through the shared host.
    Solid(String),
    /// Zero-length segment (after snapping/projection).
    DegenerateSegment,
    /// The referenced entity does not exist in the active sketch.
    EntityNotFound(EntityId),
    /// The referenced entity is not a point.
    NotAPoint(EntityId),
    NothingToUndo,
    NothingToRedo,
    /// The host supplied a non-finite or unsupported adaptive grid spacing.
    InvalidGridStep(f64),
    /// The constraint/entity-kind combination is not applicable.
    InvalidConstraint(String),
    /// Expression parse/eval failure (D9): the message is user-facing
    /// (unexpected token, unknown parameter, division by zero, cycle).
    Expression(String),
    /// Adding the constraint would over-constrain the sketch (D4.2) —
    /// rejected with the conflicting constraints named.
    OverConstrained {
        rejected: ConstraintDesc,
        conflicts_with: Vec<ConstraintDesc>,
    },
    /// The proposed relation is already implied by the existing graph. It is
    /// geometrically consistent, but contributes zero independent rank and
    /// would therefore make the sketch redundantly constrained.
    RedundantConstraint {
        rejected: ConstraintDesc,
        implied_by: Vec<ConstraintDesc>,
    },
    /// The proposed relation could not be solved, and leave-one-out analysis
    /// did not prove that any existing constraint caused the failure.
    ConstraintSolveFailed {
        rejected: ConstraintDesc,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::SketchAlreadyActive => write!(f, "a sketch is already being edited"),
            SessionError::NoActiveSketch => write!(f, "no active sketch"),
            SessionError::SketchNotFound(name) => write!(f, "finished sketch '{name}' not found"),
            SessionError::UnsupportedPlane => {
                write!(f, "plane reference is not currently resolvable")
            }
            SessionError::BrokenReference(message) => {
                write!(f, "broken reference: {message}")
            }
            SessionError::Solid(message) => write!(f, "{message}"),
            SessionError::DegenerateSegment => write!(f, "segment has zero length"),
            SessionError::EntityNotFound(id) => write!(f, "entity {} not found", id.0),
            SessionError::NotAPoint(id) => write!(f, "entity {} is not a point", id.0),
            SessionError::NothingToUndo => write!(f, "nothing to undo"),
            SessionError::NothingToRedo => write!(f, "nothing to redo"),
            SessionError::InvalidGridStep(step) => write!(
                f,
                "grid step must be between {MIN_GRID_STEP_MM} mm and {MAX_GRID_STEP_MM} mm, got {step}"
            ),
            SessionError::InvalidConstraint(msg) => write!(f, "{msg}"),
            SessionError::Expression(msg) => write!(f, "{msg}"),
            SessionError::OverConstrained {
                rejected,
                conflicts_with,
            } => {
                let ents = rejected
                    .entities
                    .iter()
                    .map(|e| e.label.as_str())
                    .collect::<Vec<_>>()
                    .join(" and ");
                if conflicts_with.is_empty() {
                    return write!(
                        f,
                        "Cannot add {} between {}: it conflicts with the existing constrained geometry",
                        rejected.kind, ents
                    );
                }
                if conflicts_with.len() > 4 {
                    return write!(
                        f,
                        "Cannot add {} between {}: conflicts with the existing constrained geometry ({} related constraints)",
                        rejected.kind,
                        ents,
                        conflicts_with.len()
                    );
                }
                let conflicts = conflicts_with
                    .iter()
                    .map(|c| {
                        let ents = c
                            .entities
                            .iter()
                            .map(|e| e.label.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{}({})", c.kind, ents)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "Cannot add {} between {}: conflicts with {}",
                    rejected.kind, ents, conflicts
                )
            }
            SessionError::RedundantConstraint {
                rejected,
                implied_by,
            } => {
                let ents = rejected
                    .entities
                    .iter()
                    .map(|entity| entity.label.as_str())
                    .collect::<Vec<_>>()
                    .join(" and ");
                if implied_by.is_empty() {
                    return write!(
                        f,
                        "Cannot add {} between {}: existing constraints already determine this relation",
                        rejected.kind, ents
                    );
                }
                if implied_by.len() > 4 {
                    return write!(
                        f,
                        "Cannot add {} between {}: this relation is already determined by {} related constraints",
                        rejected.kind,
                        ents,
                        implied_by.len()
                    );
                }
                let dependencies = implied_by
                    .iter()
                    .map(|constraint| {
                        let entities = constraint
                            .entities
                            .iter()
                            .map(|entity| entity.label.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{}({})", constraint.kind, entities)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "Cannot add {} between {}: this relation is already determined by {}",
                    rejected.kind, ents, dependencies
                )
            }
            SessionError::ConstraintSolveFailed { rejected } => {
                let ents = rejected
                    .entities
                    .iter()
                    .map(|entity| entity.label.as_str())
                    .collect::<Vec<_>>()
                    .join(" and ");
                write!(
                    f,
                    "The sketch solver could not satisfy {} between {}. No specific conflicting constraint was identified",
                    rejected.kind, ents
                )
            }
        }
    }
}

impl std::error::Error for SessionError {}

/// One undoable command: exact sketch state before/after the mutation.
/// Snapshot-based on purpose — exact by construction and covers solver
/// motion plus cascading deletes without bespoke inverse logic.
#[derive(Debug)]
struct Command {
    before: SketchSnapshot,
    after: SketchSnapshot,
}

/// Live editing session of one sketch.
#[derive(Debug)]
pub struct SketchSession {
    name: String,
    plane: PlaneRef,
    basis: PlaneBasis,
    /// Display placement only; excluded from project and undo snapshots.
    edit_placement: Option<(
        limo_cad_assembly::OccurrenceId,
        limo_cad_assembly::AssemblyTransformDto,
    )>,
    sketch: Sketch,
    grid_snap: bool,
    /// Point/origin/coincident snapping (magnet to existing geometry).
    /// The palette "Snap" toggle drives BOTH flags (all snapping off,
    /// owner M1c-ii spec); `grid_snap` alone only governs grid rounding.
    point_snap: bool,
    grid_step: f64,
    snap_tolerance: f64,
    grid_capture_mm: Option<f64>,
    /// Runtime external references derived from the support face. These are
    /// rebuilt from stable edge ids when a face-hosted sketch is opened.
    reference_midpoints: Vec<(EdgeId, Vec2)>,
    /// Support-face boundary in sketch coordinates at this history stage.
    /// Persisted so dependent profiles exist before solid replay on load.
    projected_edges: Vec<ProjectedEdgeDto>,
    /// Legacy sketches keep authored-only profiles; silently enabling boundary
    /// regions would change the numeric profile indices in saved features.
    projects_support_boundary: bool,
    profile_identities: crate::profile_identity::ProfileIdentities,
    undo: Vec<Command>,
    redo: Vec<Command>,
    /// Pre-drag snapshot captured on `DragPhase::Begin`; committed as one
    /// undoable command on `DragPhase::End`.
    pending_drag: Option<SketchSnapshot>,
    /// Last consistent state within a drag — restored when a solver-pinned
    /// update fails to converge.
    last_good_drag: Option<SketchSnapshot>,
    /// Latest solver analysis (drives DOF + fully-defined flags in DTOs).
    analysis: Option<Analysis>,
    /// Dimension annotation style from the document settings (D4.5).
    dimension_style: DimensionStyle,
}

/// Locked dynamic-input state for a commit (values evaluated, raw text
/// preserved for auto-dimension creation, D9).
#[derive(Debug, Clone, Default)]
pub(crate) struct LockedInput {
    pub length_mm: Option<f64>,
    pub angle_deg: Option<f64>,
    pub length_text: Option<String>,
    pub angle_text: Option<String>,
    pub tracking: Option<LineTrackingRequest>,
    pub intersection: Option<LineIntersectionRequest>,
    pub from_crossing: Option<CurveCrossingRequest>,
    pub to_crossing: Option<CurveCrossingRequest>,
}

/// Resolved placement of one segment endpoint: the point id to use
/// (existing or newly created) and its coordinates.
enum EndpointResolution {
    Existing(EntityId),
    New(Vec2),
}

struct ChamferEditTopology {
    origin: EntityId,
    cutbacks: [EntityId; 2],
    carriers: [EntityId; 2],
    far_points: [EntityId; 2],
}

impl SketchSession {
    pub fn new(
        name: impl Into<String>,
        plane: PlaneRef,
        basis: PlaneBasis,
        grid_snap: bool,
    ) -> Self {
        Self {
            name: name.into(),
            plane,
            basis,
            edit_placement: None,
            sketch: Sketch::new(),
            grid_snap,
            point_snap: true,
            grid_step: GRID_STEP_MM,
            snap_tolerance: SNAP_TOLERANCE_MM,
            grid_capture_mm: None,
            reference_midpoints: Vec::new(),
            projected_edges: Vec::new(),
            projects_support_boundary: matches!(plane, PlaneRef::PlanarFace { .. }),
            profile_identities: Default::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            pending_drag: None,
            last_good_drag: None,
            analysis: None,
            dimension_style: DimensionStyle::default(),
        }
    }

    pub fn set_dimension_style(&mut self, style: DimensionStyle) {
        self.dimension_style = style;
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn set_name(&mut self, name: &str) {
        self.name.clear();
        self.name.push_str(name);
    }

    pub fn plane(&self) -> PlaneRef {
        self.plane
    }

    pub(crate) fn basis(&self) -> PlaneBasis {
        self.basis
    }

    pub(crate) fn set_basis(&mut self, basis: PlaneBasis) {
        self.basis = basis;
    }

    pub(crate) fn set_edit_placement(
        &mut self,
        placement: Option<(
            limo_cad_assembly::OccurrenceId,
            limo_cad_assembly::AssemblyTransformDto,
        )>,
    ) {
        self.edit_placement = placement;
    }

    pub(crate) fn editing_occurrence(&self) -> bool {
        self.edit_placement.is_some()
    }

    fn display_basis(&self) -> PlaneBasis {
        let Some((_, pose)) = self.edit_placement else {
            return self.basis;
        };
        let rotation = limo_cad_assembly::AssemblyTransformDto {
            translation: [0.0; 3],
            rotation: pose.rotation,
        };
        PlaneBasis {
            origin: pose.transform_point(self.basis.origin),
            u: rotation.transform_point(self.basis.u),
            v: rotation.transform_point(self.basis.v),
            normal: rotation.transform_point(self.basis.normal),
        }
    }

    /// Install the current support-face edge midpoints and refresh every
    /// persistent external-midpoint constraint by stable edge id.
    ///
    /// Undo/redo snapshots are refreshed too: restoring an older command
    /// must never restore an obsolete sampled coordinate for the same edge.
    pub fn set_reference_midpoints(&mut self, midpoints: Vec<(EdgeId, Vec2)>) {
        let targets = midpoints.iter().copied().collect::<HashMap<_, _>>();
        let changed = self.sketch.refresh_reference_midpoints(&targets);
        for command in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            command.before.refresh_reference_midpoints(&targets);
            command.after.refresh_reference_midpoints(&targets);
        }
        if let Some(snapshot) = &mut self.pending_drag {
            snapshot.refresh_reference_midpoints(&targets);
        }
        if let Some(snapshot) = &mut self.last_good_drag {
            snapshot.refresh_reference_midpoints(&targets);
        }
        self.reference_midpoints = midpoints;
        if changed {
            self.recompute();
        }
    }

    /// Refresh only when the solid scene represents this sketch's history
    /// stage, never from a downstream cut that has already consumed it.
    pub fn set_projected_edges(&mut self, projected: Vec<ProjectedEdgeDto>) {
        if self.projects_support_boundary {
            let previous: std::collections::HashSet<_> = self
                .projected_edges
                .iter()
                .map(|edge| edge.edge_id)
                .collect();
            self.reference_midpoints
                .retain(|(edge, _)| !previous.contains(edge));
            self.projected_edges = projected;
            self.refresh_reference_edges();
            self.restore_projected_midpoints();
            self.refresh_profile_identities();
        }
    }

    fn restore_projected_midpoints(&mut self) {
        let mut midpoints = self.reference_midpoints.clone();
        for edge in &self.projected_edges {
            if let Some(point) = edge.midpoint() {
                if let Some((_, target)) = midpoints.iter_mut().find(|(id, _)| *id == edge.edge_id)
                {
                    *target = point;
                } else {
                    midpoints.push((edge.edge_id, point));
                }
            }
        }
        self.set_reference_midpoints(midpoints);
    }

    fn refresh_reference_edges(&mut self) {
        self.sketch.set_reference_edges(&self.projected_edges);
        for command in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            command.before.set_reference_edges(&self.projected_edges);
            command.after.set_reference_edges(&self.projected_edges);
        }
        if let Some(snapshot) = &mut self.pending_drag {
            snapshot.set_reference_edges(&self.projected_edges);
        }
        if let Some(snapshot) = &mut self.last_good_drag {
            snapshot.set_reference_edges(&self.projected_edges);
        }
        self.recompute();
    }

    pub(crate) fn projects_support_boundary(&self) -> bool {
        self.projects_support_boundary
    }

    pub fn projected_edges(&self) -> &[ProjectedEdgeDto] {
        &self.projected_edges
    }

    /// Convert a midpoint snap into the durable relation committed with the
    /// new point. Support-face midpoints use the stable OCCT edge id rather
    /// than a one-time coordinate sample.
    fn midpoint_constraint_for_target(
        &self,
        point: EntityId,
        target: SnapTarget,
    ) -> Option<Constraint> {
        match target {
            SnapTarget::ProjectedEdge { edge, .. } => {
                Some(Constraint::ReferenceOnEdge { point, edge })
            }
            SnapTarget::Midpoint { entity } => Some(Constraint::Midpoint {
                a: point,
                b: entity,
            }),
            SnapTarget::ReferenceMidpoint { edge } => {
                let position = self
                    .reference_midpoints
                    .iter()
                    .find_map(|(candidate, position)| (*candidate == edge).then_some(*position))
                    .or_else(|| self.sketch.point_position(point))?;
                Some(Constraint::ReferenceMidpoint {
                    point,
                    edge,
                    position,
                })
            }
            _ => None,
        }
    }

    pub fn sketch(&self) -> &Sketch {
        &self.sketch
    }

    pub fn set_grid_snap(&mut self, enabled: bool) {
        self.grid_snap = enabled;
        self.point_snap = enabled;
    }

    pub fn set_grid_step(&mut self, step_mm: f64) -> Result<(), SessionError> {
        if !step_mm.is_finite() || !(MIN_GRID_STEP_MM..=MAX_GRID_STEP_MM).contains(&step_mm) {
            return Err(SessionError::InvalidGridStep(step_mm));
        }
        self.grid_step = step_mm;
        Ok(())
    }

    pub(crate) fn replace_viewport_snap(
        &mut self,
        context: crate::dto::ViewportSnapContext,
    ) -> Result<(f64, f64, Option<f64>), SessionError> {
        if !context.point_tolerance_mm.is_finite()
            || context.point_tolerance_mm <= 0.
            || !context.grid_capture_mm.is_finite()
            || context.grid_capture_mm <= 0.
        {
            return Err(SessionError::Solid(
                "Viewport snap distances must be finite and positive".into(),
            ));
        }
        let previous = (self.grid_step, self.snap_tolerance, self.grid_capture_mm);
        self.set_grid_step(context.grid_step_mm)?;
        self.snap_tolerance = context.point_tolerance_mm;
        self.grid_capture_mm = Some(context.grid_capture_mm);
        Ok(previous)
    }

    pub(crate) fn restore_viewport_snap(&mut self, previous: (f64, f64, Option<f64>)) {
        (self.grid_step, self.snap_tolerance, self.grid_capture_mm) = previous;
    }

    fn grid_capture_distance(&self) -> f64 {
        self.grid_capture_mm
            .unwrap_or(self.grid_step * GRID_CAPTURE_FRACTION)
    }

    pub fn preview_creation_point(
        &self,
        request: crate::dto::CreationPointPreviewRequest,
    ) -> PreviewDto {
        let (snapped_to, snap) = self.snap_inner(
            request.raw,
            !request.ctrl_held,
            request.allow_midpoint && !request.ctrl_held,
            request.exclude_position,
        );
        PreviewDto {
            snapped_to,
            snap,
            inferences: vec![],
            tracking: None,
        }
    }

    pub(crate) fn project_state(&self, feature_id: limo_cad_core::FeatureId) -> ProjectSketchV2 {
        ProjectSketchV2 {
            feature_id,
            name: self.name.clone(),
            plane: self.plane,
            basis: self.basis,
            dimension_style: self.dimension_style,
            grid_snap: self.grid_snap,
            snapshot: self.sketch.snapshot(),
            entity_id_high_water: self.sketch.entity_id_high_water(),
            profile_identities: self.profile_identities.clone(),
            support_boundary: self
                .projects_support_boundary
                .then(|| self.projected_edges.clone()),
        }
    }

    pub(crate) fn from_project_state(state: ProjectSketchV2) -> Result<Self, SessionError> {
        state
            .snapshot
            .validate()
            .map_err(|error| SessionError::Solid(format!("invalid saved sketch: {error}")))?;
        let mut sketch = Sketch::new();
        sketch.restore(state.snapshot);
        sketch.reserve_entity_ids(state.entity_id_high_water);
        let mut session = Self {
            name: state.name,
            plane: state.plane,
            basis: state.basis,
            edit_placement: None,
            sketch,
            grid_snap: state.grid_snap,
            point_snap: state.grid_snap,
            grid_step: GRID_STEP_MM,
            snap_tolerance: SNAP_TOLERANCE_MM,
            grid_capture_mm: None,
            reference_midpoints: Vec::new(),
            projects_support_boundary: state.support_boundary.is_some(),
            profile_identities: state.profile_identities,
            projected_edges: state.support_boundary.unwrap_or_default(),
            undo: Vec::new(),
            redo: Vec::new(),
            pending_drag: None,
            last_good_drag: None,
            analysis: None,
            dimension_style: state.dimension_style,
        };
        if session.projects_support_boundary {
            session.refresh_reference_edges();
            session.restore_projected_midpoints();
        }

        session.attach_missing_circle_centers();
        session.recompute();
        session.refresh_profile_identities();
        Ok(session)
    }

    pub(crate) fn refresh_profile_identities(&mut self) {
        let sketch = self.dto();
        let mut catalog =
            crate::manager::profile_catalog_item(&sketch, limo_cad_core::FeatureId(0));
        self.profile_identities
            .assign(&sketch, &mut catalog.profiles);
    }

    pub(crate) fn profile_catalog(
        &self,
        feature_id: limo_cad_core::FeatureId,
    ) -> limo_cad_solid::ProfileCatalogItemDto {
        let sketch = self.dto();
        let mut catalog = crate::manager::profile_catalog_item(&sketch, feature_id);
        self.profile_identities
            .clone()
            .assign(&sketch, &mut catalog.profiles);
        if self.sketch.constraints().any(|(id, c)| {
            matches!(c, Constraint::ReferenceOnEdge { edge, .. }
            if self.sketch.reference_edge(*edge).is_none()
                || solver::constraint_residual(&self.sketch, id) > INCONSISTENT_EPS)
        }) {
            catalog.profiles.clear();
            catalog.profile_error = Some(
                "Broken support-edge reference; repair or remove the affected relation".into(),
            );
        }
        catalog
    }

    /// Wrapper so the sibling `mods` module can create dimension params.
    pub(crate) fn param_from_text_pub(
        &mut self,
        kind: crate::params::ParamKind,
        text: Option<&str>,
        measured: f64,
    ) -> Result<crate::params::ParamId, SessionError> {
        self.param_from_text(kind, text, measured)
    }

    /// Re-solve after a mutation and refresh the analysis used by DTOs.
    fn recompute(&mut self) {
        self.analysis = Some(solver::solve(&mut self.sketch, &[]));
        self.sketch.sync_dimension_constraint_values();
    }

    fn analysis(&self) -> Analysis {
        self.analysis
            .clone()
            .unwrap_or_else(|| solver::analyze(&self.sketch))
    }

    /// Snap priority: existing point within tolerance > origin within
    /// tolerance > line midpoint (`allow_midpoint`, line flow only) > grid
    /// intersection (when grid snap on) > raw. Point, origin, and midpoint
    /// snaps are governed by `point_snap`, grid rounding by `grid_snap`.
    fn snap_inner(
        &self,
        raw: Vec2,
        allow_relational: bool,
        allow_midpoint: bool,
        exclude_position: Option<Vec2>,
    ) -> (Vec2, SnapTarget) {
        if self.point_snap && allow_relational {
            if let Some((id, _)) = self
                .sketch
                .entities()
                .filter_map(|(id, entity)| {
                    let Entity::Point { position } = entity else {
                        return None;
                    };
                    if exclude_position
                        .is_some_and(|excluded| position.distance(excluded) <= MERGE_EPS)
                    {
                        return None;
                    }
                    let distance = position.distance(raw);
                    (distance <= self.snap_tolerance).then_some((id, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                let position = self.sketch.point_position(id).unwrap_or(raw);
                return (position, SnapTarget::Point { entity: id });
            }
            if raw.distance(Vec2::ZERO) <= self.snap_tolerance
                && !exclude_position
                    .is_some_and(|excluded| excluded.distance(Vec2::ZERO) <= MERGE_EPS)
            {
                return (Vec2::ZERO, SnapTarget::Origin);
            }
            if allow_midpoint {
                if let Some((id, mid)) = self.sketch.nearest_line_midpoint(raw, self.snap_tolerance)
                {
                    if !exclude_position.is_some_and(|excluded| mid.distance(excluded) <= MERGE_EPS)
                    {
                        return (mid, SnapTarget::Midpoint { entity: id });
                    }
                }
                if let Some((edge, midpoint, _)) = self
                    .reference_midpoints
                    .iter()
                    .filter_map(|(edge, midpoint)| {
                        if exclude_position
                            .is_some_and(|excluded| midpoint.distance(excluded) <= MERGE_EPS)
                        {
                            return None;
                        }
                        let distance = midpoint.distance(raw);
                        (distance <= self.snap_tolerance).then_some((*edge, *midpoint, distance))
                    })
                    .min_by(|a, b| a.2.total_cmp(&b.2))
                {
                    return (midpoint, SnapTarget::ReferenceMidpoint { edge });
                }
                if let Some((edge, position)) = self.nearest_projected_edge_point(raw) {
                    if !exclude_position
                        .is_some_and(|excluded| position.distance(excluded) <= MERGE_EPS)
                    {
                        return (position, SnapTarget::ProjectedEdge { edge, position });
                    }
                }
            }
        }

        if self.grid_snap {
            let step = self.grid_step;
            let snapped = Vec2::new((raw.x / step).round() * step, (raw.y / step).round() * step);
            if exclude_position.is_some_and(|excluded| snapped.distance(excluded) <= MERGE_EPS) {
                return (raw, SnapTarget::None);
            }
            if snapped.distance(raw) <= self.grid_capture_distance() {
                return (snapped, SnapTarget::Grid);
            }
        }
        (raw, SnapTarget::None)
    }

    fn snap(&self, raw: Vec2) -> (Vec2, SnapTarget) {
        self.snap_inner(raw, true, false, None)
    }

    /// Nearest point on the projected support-face boundary within snap range.
    ///
    /// This is what lets a line or arc endpoint land exactly on the face edge
    /// the sketch was created from: the projection is reference geometry, so
    /// creation tools persist the acquired edge as a sliding reference.
    fn nearest_projected_edge_point(&self, raw: Vec2) -> Option<(EdgeId, Vec2)> {
        self.projected_edges
            .iter()
            .filter_map(|edge| {
                let position = edge.closest_point(raw)?;
                let distance = position.distance(raw);
                (distance <= self.snap_tolerance).then_some((edge.edge_id, position, distance))
            })
            .min_by(|left, right| left.2.total_cmp(&right.2))
            .map(|(edge_id, position, _)| (edge_id, position))
    }

    /// Creation-tool snap with a temporary inference override. Ctrl/Cmd
    /// suppresses object/datum acquisition while leaving the independently
    /// configured engineering grid available.
    /// The one creation snap every curve/rectangle create tool uses: points,
    /// origin, curve crossings, line midpoints, support-face edge midpoints and
    /// the projected face boundary. Keeping a single implementation means the
    /// cursor, the preview and the commit agree about what a pick acquired.
    /// Holding Ctrl suppresses the whole acquisition.
    fn snap_creation(&self, raw: Vec2, ctrl_held: bool) -> (Vec2, SnapTarget) {
        self.snap_inner(raw, !ctrl_held, !ctrl_held, None)
    }

    /// Creation snap without midpoint-class references. The Point tool is the
    /// only caller: it resolves curve interiors, virtual line extensions and
    /// carrier coincidences itself, and a midpoint reference would override
    /// that choice.
    fn snap_creation_carrier_only(&self, raw: Vec2, ctrl_held: bool) -> (Vec2, SnapTarget) {
        let snapped = self.snap_inner(raw, !ctrl_held, false, None);
        if !ctrl_held
            && self.point_snap
            && !matches!(snapped.1, SnapTarget::Point { .. } | SnapTarget::Origin)
        {
            if let Some((edge, position)) = self.nearest_projected_edge_point(raw) {
                return (position, SnapTarget::ProjectedEdge { edge, position });
            }
        }
        snapped
    }

    /// Line-flow snap (M1d): midpoint snapping is enabled here only, because
    /// the line flow is the one that also auto-creates the matching Midpoint
    /// constraint on commit (D4.1 parity). Holding Ctrl suppresses the
    /// midpoint inference (Ctrl disables inferences).
    fn snap_line_flow(&self, raw: Vec2, ctrl_held: bool) -> (Vec2, SnapTarget) {
        self.snap_inner(raw, !ctrl_held, !ctrl_held, None)
    }

    /// Endpoint acquisition must not magnetize a short segment back onto its
    /// own start point. Other coincident candidates retain normal priority.
    fn snap_line_endpoint(&self, raw: Vec2, from: Vec2, ctrl_held: bool) -> (Vec2, SnapTarget) {
        self.snap_inner(raw, !ctrl_held, !ctrl_held, Some(from))
    }

    /// Shared pipeline for `preview_segment` and `add_line`: snap, then
    /// (unless Ctrl is held or the snap is coincident) apply H/V inference
    /// within `INFERENCE_ANGLE_TOL_DEG` of the u/v axes and project the
    /// endpoint onto the inferred direction.
    fn snap_and_infer(&self, from: Vec2, to_raw: Vec2, ctrl_held: bool) -> PreviewDto {
        let (mut snapped, mut target) = self.snap_line_endpoint(to_raw, from, ctrl_held);
        let mut inferences = Vec::new();

        match target {
            SnapTarget::Point { .. } | SnapTarget::Origin => {
                inferences.push(Inference::Coincident);
            }
            SnapTarget::Midpoint { .. }
            | SnapTarget::ReferenceMidpoint { .. }
            | SnapTarget::ProjectedEdge { .. }
            | SnapTarget::Curve { .. }
            | SnapTarget::Intersection { .. } => {}
            SnapTarget::Grid | SnapTarget::None => {
                if !ctrl_held {
                    let d = to_raw - from;
                    let tol = INFERENCE_ANGLE_TOL_DEG.to_radians().tan();
                    if d.x.abs() >= d.y.abs() && d.y.abs() <= tol * d.x.abs() {
                        snapped.y = from.y;

                        if snapped.distance(from) < MIN_LINE_LENGTH_MM
                            && d.length() >= MIN_LINE_LENGTH_MM
                        {
                            snapped.x = to_raw.x;
                            target = SnapTarget::None;
                        }
                        inferences.push(Inference::Horizontal);
                    } else if d.y.abs() > d.x.abs() && d.x.abs() <= tol * d.y.abs() {
                        snapped.x = from.x;
                        if snapped.distance(from) < MIN_LINE_LENGTH_MM
                            && d.length() >= MIN_LINE_LENGTH_MM
                        {
                            snapped.y = to_raw.y;
                            target = SnapTarget::None;
                        }
                        inferences.push(Inference::Vertical);
                    }
                }
            }
        }

        if !ctrl_held
            && snapped.distance(from) >= MIN_LINE_LENGTH_MM
            && self.preview_has_connected_perpendicular(from, snapped, target)
        {
            inferences.push(Inference::Perpendicular);
        }

        PreviewDto {
            snapped_to: snapped,
            snap: target,
            inferences,
            tracking: None,
        }
    }

    /// Preview with optional locked fields. Locks constrain only their own
    /// DOF (D9): point snapping and H/V inference keep working on the
    /// remaining freedom (endpoint on the locked circle/ray, still
    /// snapping to points/axes along it).
    pub fn preview_segment_locked(
        &self,
        from: Vec2,
        (length_mm, angle_deg): (Option<f64>, Option<f64>),
        to_hint: Vec2,
        ctrl_held: bool,
        (tracking, intersection, from_crossing, to_crossing): (
            Option<LineTrackingRequest>,
            Option<LineIntersectionRequest>,
            Option<CurveCrossingRequest>,
            Option<CurveCrossingRequest>,
        ),
    ) -> PreviewDto {
        let from = from_crossing
            .and_then(|request| self.curve_crossing_point(request, from))
            .unwrap_or(from);
        if !ctrl_held {
            if let Some(preview) = self
                .curve_crossing_preview(to_hint, to_crossing)
                .filter(|p| {
                    let delta = p.snapped_to - from;
                    length_mm.is_none_or(|l| (delta.length() - l).abs() <= MERGE_EPS)
                        && angle_deg.is_none_or(|a| {
                            let direction = Vec2::new(a.to_radians().cos(), a.to_radians().sin());
                            delta.dot(direction) >= 0.0
                                && delta.dot(direction.perp()).abs() <= MERGE_EPS
                        })
                })
            {
                return preview;
            }
        }
        if !ctrl_held {
            if let Some(preview) = self.curve_axis_preview(
                from,
                length_mm,
                angle_deg.map(f64::to_radians),
                to_hint,
                intersection,
            ) {
                return preview;
            }
        }
        if length_mm.is_none() && angle_deg.is_none() {
            if !ctrl_held {
                if let Some(preview) = self.tracking_preview(from, None, None, to_hint, tracking) {
                    return preview;
                }
            }
            return self.snap_and_infer(from, to_hint, ctrl_held);
        }
        let angle = angle_deg.map(|a| a.to_radians());
        let mut inferences = Vec::new();

        if let (Some(l), Some(a)) = (length_mm, angle) {
            let exact = from + Vec2::new(a.cos() * l, a.sin() * l);
            return self.coincident_or_exact(from, exact, inferences, ctrl_held);
        }

        let endpoint = if let Some(l) = length_mm {
            if let Some((id, pos)) = (!ctrl_held)
                .then(|| self.point_on_circle_locus(from, l, to_hint))
                .flatten()
            {
                inferences.push(Inference::Coincident);
                return PreviewDto {
                    snapped_to: pos,
                    snap: SnapTarget::Point { entity: id },
                    inferences,
                    tracking: None,
                };
            }
            if !ctrl_held {
                if let Some(preview) = self.tracking_preview(from, Some(l), None, to_hint, tracking)
                {
                    return preview;
                }
            }

            let d = to_hint - from;
            let tol = INFERENCE_ANGLE_TOL_DEG.to_radians().tan();
            if !ctrl_held && d.x.abs() >= d.y.abs() && d.y.abs() <= tol * d.x.abs() {
                inferences.push(Inference::Horizontal);
                Vec2::new(from.x + l * d.x.signum(), from.y)
            } else if !ctrl_held && d.y.abs() > d.x.abs() && d.x.abs() <= tol * d.y.abs() {
                inferences.push(Inference::Vertical);
                Vec2::new(from.x, from.y + l * d.y.signum())
            } else {
                if let Some(grid) = self.point_on_circle_grid(from, l, to_hint) {
                    return PreviewDto {
                        snapped_to: grid,
                        snap: SnapTarget::Grid,
                        inferences,
                        tracking: None,
                    };
                }

                let len = d.length();
                if len < MERGE_EPS {
                    from + Vec2::new(l, 0.0)
                } else {
                    from + d * (l / len)
                }
            }
        } else if let Some(a) = angle {
            let dir = Vec2::new(a.cos(), a.sin());

            if let Some((id, pos)) = (!ctrl_held)
                .then(|| self.point_on_ray_locus(from, dir, to_hint))
                .flatten()
            {
                inferences.push(Inference::Coincident);
                return PreviewDto {
                    snapped_to: pos,
                    snap: SnapTarget::Point { entity: id },
                    inferences,
                    tracking: None,
                };
            }
            if !ctrl_held {
                if let Some(preview) = self.tracking_preview(from, None, Some(a), to_hint, tracking)
                {
                    return preview;
                }
            }

            if let Some(grid) = self.point_on_ray_grid(from, dir, to_hint) {
                return PreviewDto {
                    snapped_to: grid,
                    snap: SnapTarget::Grid,
                    inferences,
                    tracking: None,
                };
            }

            let t = (to_hint - from).dot(dir).max(0.0);
            from + dir * t
        } else {
            unreachable!()
        };

        self.coincident_or_exact(from, endpoint, inferences, ctrl_held)
    }

    /// Analytic intersections of two finite sketch curves. Screen-space
    /// acquisition selects the carrier ids; this engine-side routine is the
    /// sole source of the committed coordinate. Unsupported spline crossings
    /// intentionally return no result instead of accepting a tessellation
    /// approximation as topology.
    fn curve_crossing_points(&self, request: CurveCrossingRequest) -> Vec<Vec2> {
        use crate::geomops::trimext::{self, Circle, LineSeg};

        #[derive(Clone, Copy)]
        enum AnalyticCurve {
            Line(LineSeg),
            Circular {
                circle: Circle,
                arc: Option<(f64, f64)>,
            },
        }

        let analytic = |id: EntityId| -> Option<AnalyticCurve> {
            match self.sketch.entity(id)? {
                Entity::Line { .. } => {
                    let (a, b) = self.sketch.resolved_line(id)?;
                    Some(AnalyticCurve::Line(LineSeg { a, b }))
                }
                Entity::Circle { center, radius } => Some(AnalyticCurve::Circular {
                    circle: Circle {
                        center: *center,
                        radius: *radius,
                    },
                    arc: None,
                }),
                Entity::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } => Some(AnalyticCurve::Circular {
                    circle: Circle {
                        center: *center,
                        radius: *radius,
                    },
                    arc: Some((*start_angle, *end_angle)),
                }),
                Entity::Point { .. } | Entity::Spline { .. } => None,
            }
        };
        let on_arc = |point: Vec2, circle: Circle, arc: Option<(f64, f64)>| {
            let Some((start, end)) = arc else {
                return true;
            };
            let sweep = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
            let angle = (point.y - circle.center.y).atan2(point.x - circle.center.x);
            sweep(start, angle) <= sweep(start, end) + 1.0e-9
        };

        if request.first == request.second {
            return Vec::new();
        }
        let (Some(first), Some(second)) = (analytic(request.first), analytic(request.second))
        else {
            return Vec::new();
        };
        let mut points = match (first, second) {
            (AnalyticCurve::Line(a), AnalyticCurve::Line(b)) => trimext::line_line(&a, &b)
                .filter(|(_, ta, tb)| {
                    (-MERGE_EPS..=1.0 + MERGE_EPS).contains(ta)
                        && (-MERGE_EPS..=1.0 + MERGE_EPS).contains(tb)
                })
                .map(|(point, _, _)| vec![point])
                .unwrap_or_default(),
            (AnalyticCurve::Line(line), AnalyticCurve::Circular { circle, arc })
            | (AnalyticCurve::Circular { circle, arc }, AnalyticCurve::Line(line)) => {
                trimext::line_circle(&line, &circle)
                    .into_iter()
                    .filter(|(point, parameter)| {
                        (-MERGE_EPS..=1.0 + MERGE_EPS).contains(parameter)
                            && on_arc(*point, circle, arc)
                    })
                    .map(|(point, _)| point)
                    .collect()
            }
            (
                AnalyticCurve::Circular {
                    circle: first_circle,
                    arc: first_arc,
                },
                AnalyticCurve::Circular {
                    circle: second_circle,
                    arc: second_arc,
                },
            ) => trimext::circle_circle(&first_circle, &second_circle)
                .into_iter()
                .filter(|point| {
                    on_arc(*point, first_circle, first_arc)
                        && on_arc(*point, second_circle, second_arc)
                })
                .collect(),
        };
        points.retain(|point| point.x.is_finite() && point.y.is_finite());
        points.dedup_by(|a, b| a.distance(*b) <= MERGE_EPS);
        points
    }

    fn curve_crossing_point(&self, request: CurveCrossingRequest, hint: Vec2) -> Option<Vec2> {
        self.curve_crossing_points(request)
            .into_iter()
            .min_by(|a, b| a.distance(hint).total_cmp(&b.distance(hint)))
    }

    fn curve_crossing_preview(
        &self,
        hint: Vec2,
        request: Option<CurveCrossingRequest>,
    ) -> Option<PreviewDto> {
        let request = request?;
        let snapped = self.curve_crossing_point(request, hint)?;
        Some(PreviewDto {
            snapped_to: snapped,
            snap: SnapTarget::Intersection {
                first: request.first,
                second: request.second,
            },
            inferences: vec![Inference::Coincident],
            tracking: None,
        })
    }

    /// Resolve a viewport-acquired horizontal/vertical tracking reference
    /// against the segment's remaining degrees of freedom. The viewport
    /// decides *which* point is close in screen space; this engine function
    /// performs the exact intersection and reports a temporary dotted guide.
    /// The aligned coordinate is committed, but no point-pair relation is
    /// inferred from this visual aid.
    fn tracking_preview(
        &self,
        from: Vec2,
        length_mm: Option<f64>,
        angle_rad: Option<f64>,
        cursor: Vec2,
        request: Option<LineTrackingRequest>,
    ) -> Option<PreviewDto> {
        let request = request?;
        let source = self.sketch.point_position(request.point)?;
        if length_mm.is_some() && angle_rad.is_some() {
            return None;
        }

        let mut grid_acquired = false;
        let snapped = match (length_mm, angle_rad, request.axis) {
            (None, None, TrackingAxis::Horizontal) => Vec2::new(
                self.snap_1d_with_status(cursor.x)
                    .map_or(cursor.x, |value| {
                        grid_acquired = true;
                        value
                    }),
                source.y,
            ),
            (None, None, TrackingAxis::Vertical) => Vec2::new(
                source.x,
                self.snap_1d_with_status(cursor.y)
                    .map_or(cursor.y, |value| {
                        grid_acquired = true;
                        value
                    }),
            ),
            (None, Some(angle), axis) => {
                let direction = Vec2::new(angle.cos(), angle.sin());
                self.ray_axis_intersection(from, direction, source, axis)?
            }
            (Some(length), None, axis) => {
                self.circle_axis_intersection(from, length, source, axis, cursor)?
            }
            (Some(_), Some(_), _) => return None,
        };
        if !snapped.x.is_finite()
            || !snapped.y.is_finite()
            || snapped.distance(from) < MIN_LINE_LENGTH_MM
        {
            return None;
        }
        Some(PreviewDto {
            snapped_to: snapped,
            snap: if grid_acquired {
                SnapTarget::Grid
            } else {
                SnapTarget::None
            },
            inferences: Vec::new(),
            tracking: Some(TrackingGuideDto {
                point: request.point,
                axis: request.axis,
                source,
                snapped_to: snapped,
            }),
        })
    }

    /// Resolve a viewport-acquired H/V intent against an existing curve.
    /// Curve geometry wins over the grid: the endpoint is recomputed from
    /// authoritative sketch entities and commit records point-on-curve.
    fn curve_axis_preview(
        &self,
        from: Vec2,
        length_mm: Option<f64>,
        angle_rad: Option<f64>,
        cursor: Vec2,
        request: Option<LineIntersectionRequest>,
    ) -> Option<PreviewDto> {
        let request = request?;
        if length_mm.is_some() {
            return None;
        }

        if let Some(angle) = angle_rad {
            let direction = Vec2::new(angle.cos(), angle.sin());
            let axis_error = match request.axis {
                TrackingAxis::Horizontal => direction.y.abs(),
                TrackingAxis::Vertical => direction.x.abs(),
            };
            if axis_error > 1.0e-7 {
                return None;
            }
        }

        let cursor_delta = cursor - from;
        let direction_sign = match request.axis {
            TrackingAxis::Horizontal => cursor_delta.x.signum(),
            TrackingAxis::Vertical => cursor_delta.y.signum(),
        };
        if direction_sign == 0.0 {
            return None;
        }

        let mut candidates = Vec::with_capacity(2);
        match self.sketch.entity(request.curve)? {
            Entity::Line { .. } => {
                let (start, end) = self.sketch.resolved_line(request.curve)?;
                let delta = end - start;
                let parameter = match request.axis {
                    TrackingAxis::Horizontal if delta.y.abs() > MERGE_EPS => {
                        (from.y - start.y) / delta.y
                    }
                    TrackingAxis::Vertical if delta.x.abs() > MERGE_EPS => {
                        (from.x - start.x) / delta.x
                    }
                    _ => return None,
                };
                if (-MERGE_EPS..=1.0 + MERGE_EPS).contains(&parameter) {
                    let point = start + delta * parameter.clamp(0.0, 1.0);
                    candidates.push(point);
                }
            }
            Entity::Circle { center, radius } | Entity::Arc { center, radius, .. } => {
                let fixed = match request.axis {
                    TrackingAxis::Horizontal => from.y - center.y,
                    TrackingAxis::Vertical => from.x - center.x,
                };
                if fixed.abs() > *radius + MERGE_EPS {
                    return None;
                }
                let free = (radius * radius - fixed * fixed).max(0.0).sqrt();
                match request.axis {
                    TrackingAxis::Horizontal => {
                        candidates.push(Vec2::new(center.x + free, from.y));
                        candidates.push(Vec2::new(center.x - free, from.y));
                    }
                    TrackingAxis::Vertical => {
                        candidates.push(Vec2::new(from.x, center.y + free));
                        candidates.push(Vec2::new(from.x, center.y - free));
                    }
                }
                if let Entity::Arc {
                    start_angle,
                    end_angle,
                    ..
                } = self.sketch.entity(request.curve)?
                {
                    let sweep =
                        |start: f64, end: f64| (end - start).rem_euclid(std::f64::consts::TAU);
                    candidates.retain(|point| {
                        let angle = (point.y - center.y).atan2(point.x - center.x);
                        sweep(*start_angle, angle) <= sweep(*start_angle, *end_angle) + 1.0e-9
                    });
                }
            }
            Entity::Point { .. } | Entity::Spline { .. } => return None,
        }

        candidates.retain(|point| {
            if point.distance(from) < MIN_LINE_LENGTH_MM {
                return false;
            }
            let travel = match request.axis {
                TrackingAxis::Horizontal => point.x - from.x,
                TrackingAxis::Vertical => point.y - from.y,
            };
            travel * direction_sign >= -MERGE_EPS
        });
        let snapped = candidates
            .into_iter()
            .min_by(|a, b| a.distance(cursor).total_cmp(&b.distance(cursor)))?;

        let mut inferences = Vec::with_capacity(2);
        if angle_rad.is_none() {
            inferences.push(match request.axis {
                TrackingAxis::Horizontal => Inference::Horizontal,
                TrackingAxis::Vertical => Inference::Vertical,
            });
        }
        inferences.push(Inference::Coincident);

        if let Some((entity, _)) = self.sketch.nearest_point(snapped, MERGE_EPS) {
            return Some(PreviewDto {
                snapped_to: self.sketch.point_position(entity).unwrap_or(snapped),
                snap: SnapTarget::Point { entity },
                inferences,
                tracking: None,
            });
        }

        Some(PreviewDto {
            snapped_to: snapped,
            snap: SnapTarget::Curve {
                entity: request.curve,
            },
            inferences,
            tracking: None,
        })
    }

    fn ray_axis_intersection(
        &self,
        from: Vec2,
        direction: Vec2,
        source: Vec2,
        axis: TrackingAxis,
    ) -> Option<Vec2> {
        let t = match axis {
            TrackingAxis::Horizontal if direction.y.abs() > MERGE_EPS => {
                (source.y - from.y) / direction.y
            }
            TrackingAxis::Vertical if direction.x.abs() > MERGE_EPS => {
                (source.x - from.x) / direction.x
            }
            _ => return None,
        };
        (t >= -MERGE_EPS).then_some(from + direction * t.max(0.0))
    }

    fn circle_axis_intersection(
        &self,
        from: Vec2,
        radius: f64,
        source: Vec2,
        axis: TrackingAxis,
        cursor: Vec2,
    ) -> Option<Vec2> {
        let (fixed_delta, first, second) = match axis {
            TrackingAxis::Horizontal => {
                let dy = source.y - from.y;
                let free = (radius * radius - dy * dy).max(0.0).sqrt();
                if dy.abs() > radius + MERGE_EPS {
                    return None;
                }
                (
                    dy,
                    Vec2::new(from.x + free, source.y),
                    Vec2::new(from.x - free, source.y),
                )
            }
            TrackingAxis::Vertical => {
                let dx = source.x - from.x;
                let free = (radius * radius - dx * dx).max(0.0).sqrt();
                if dx.abs() > radius + MERGE_EPS {
                    return None;
                }
                (
                    dx,
                    Vec2::new(source.x, from.y + free),
                    Vec2::new(source.x, from.y - free),
                )
            }
        };
        if !fixed_delta.is_finite() {
            return None;
        }
        Some(if first.distance(cursor) <= second.distance(cursor) {
            first
        } else {
            second
        })
    }

    /// Nearest intersection of a locked ray and either family of active grid
    /// lines. This preserves the typed angle exactly while snapping its one
    /// remaining degree of freedom to engineering increments.
    fn point_on_ray_grid(&self, from: Vec2, direction: Vec2, cursor: Vec2) -> Option<Vec2> {
        if !self.grid_snap {
            return None;
        }
        let step = self.grid_step;
        let mut candidates = Vec::with_capacity(2);
        if direction.x.abs() > MERGE_EPS {
            let x = (cursor.x / step).round() * step;
            let t = (x - from.x) / direction.x;
            if t >= -MERGE_EPS {
                candidates.push(from + direction * t.max(0.0));
            }
        }
        if direction.y.abs() > MERGE_EPS {
            let y = (cursor.y / step).round() * step;
            let t = (y - from.y) / direction.y;
            if t >= -MERGE_EPS {
                candidates.push(from + direction * t.max(0.0));
            }
        }
        candidates
            .into_iter()
            .filter(|point| point.distance(cursor) <= self.grid_capture_distance())
            .min_by(|a, b| a.distance(cursor).total_cmp(&b.distance(cursor)))
    }

    /// Nearest intersection of a locked-length circle and the active grid.
    /// Sampling the nearest grid line plus its neighbours handles cursors
    /// outside the circle without dropping a valid nearby crossing.
    fn point_on_circle_grid(&self, from: Vec2, radius: f64, cursor: Vec2) -> Option<Vec2> {
        if !self.grid_snap || radius <= MIN_LINE_LENGTH_MM {
            return None;
        }
        let step = self.grid_step;
        let mut candidates = Vec::with_capacity(12);
        for offset in -1..=1 {
            let x = (cursor.x / step).round() * step + f64::from(offset) * step;
            let dx = x - from.x;
            if dx.abs() <= radius + MERGE_EPS {
                let dy = (radius * radius - dx * dx).max(0.0).sqrt();
                candidates.push(Vec2::new(x, from.y + dy));
                candidates.push(Vec2::new(x, from.y - dy));
            }
            let y = (cursor.y / step).round() * step + f64::from(offset) * step;
            let dy = y - from.y;
            if dy.abs() <= radius + MERGE_EPS {
                let dx = (radius * radius - dy * dy).max(0.0).sqrt();
                candidates.push(Vec2::new(from.x + dx, y));
                candidates.push(Vec2::new(from.x - dx, y));
            }
        }
        candidates
            .into_iter()
            .filter(|point| point.distance(cursor) <= self.grid_capture_distance())
            .min_by(|a, b| a.distance(cursor).total_cmp(&b.distance(cursor)))
    }

    /// Coincident-merge check for a computed endpoint (point entities,
    /// then the origin), else the point itself as a free snap.
    fn coincident_or_exact(
        &self,
        from: Vec2,
        exact: Vec2,
        mut inferences: Vec<Inference>,
        ctrl_held: bool,
    ) -> PreviewDto {
        if !self.point_snap || ctrl_held {
            return PreviewDto {
                snapped_to: exact,
                snap: SnapTarget::None,
                inferences,
                tracking: None,
            };
        }
        if let Some((id, _)) = self
            .sketch
            .entities()
            .filter_map(|(id, entity)| {
                let Entity::Point { position } = entity else {
                    return None;
                };
                if position.distance(from) <= MERGE_EPS {
                    return None;
                }
                let distance = position.distance(exact);
                (distance <= MERGE_EPS).then_some((id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            inferences.push(Inference::Coincident);
            return PreviewDto {
                snapped_to: self.sketch.point_position(id).unwrap_or(exact),
                snap: SnapTarget::Point { entity: id },
                inferences,
                tracking: None,
            };
        }
        if exact.distance(Vec2::ZERO) <= MERGE_EPS && from.distance(Vec2::ZERO) > MERGE_EPS {
            inferences.push(Inference::Coincident);
            return PreviewDto {
                snapped_to: Vec2::ZERO,
                snap: SnapTarget::Origin,
                inferences,
                tracking: None,
            };
        }
        PreviewDto {
            snapped_to: exact,
            snap: SnapTarget::None,
            inferences,
            tracking: None,
        }
    }

    /// Nearest existing point lying on the locked circle within snap tol
    /// of the locus and within ~4×tol of the cursor (locality gate).
    fn point_on_circle_locus(&self, from: Vec2, l: f64, cursor: Vec2) -> Option<(EntityId, Vec2)> {
        if !self.point_snap {
            return None;
        }
        let mut best: Option<(EntityId, f64)> = None;
        for (id, e) in self.sketch.entities() {
            let Entity::Point { position } = e else {
                continue;
            };
            if position.distance(from) <= MERGE_EPS {
                continue;
            }
            if (position.distance(from) - l).abs() > MERGE_EPS {
                continue;
            }
            let dc = position.distance(cursor);
            if dc > self.snap_tolerance * 4.0 {
                continue;
            }
            if best.is_none_or(|(_, bd)| dc < bd) {
                best = Some((id, dc));
            }
        }
        best.and_then(|(id, _)| self.sketch.point_position(id).map(|p| (id, p)))
    }

    /// Nearest existing point lying on the locked ray (perpendicular
    /// deviation within tol, not behind the origin).
    fn point_on_ray_locus(&self, from: Vec2, dir: Vec2, cursor: Vec2) -> Option<(EntityId, Vec2)> {
        if !self.point_snap {
            return None;
        }
        let mut best: Option<(EntityId, f64)> = None;
        for (id, e) in self.sketch.entities() {
            let Entity::Point { position } = e else {
                continue;
            };
            if position.distance(from) <= MERGE_EPS {
                continue;
            }
            let rel = *position - from;
            if (rel.x * dir.y - rel.y * dir.x).abs() > MERGE_EPS {
                continue;
            }
            if rel.dot(dir) < -MERGE_EPS {
                continue;
            }
            let dc = position.distance(cursor);
            if dc > self.snap_tolerance * 4.0 {
                continue;
            }
            if best.is_none_or(|(_, bd)| dc < bd) {
                best = Some((id, dc));
            }
        }
        best.and_then(|(id, _)| self.sketch.point_position(id).map(|p| (id, p)))
    }

    /// Resolve one segment endpoint to an existing point id or a new point
    /// location. Merging (structural coincident) happens here: endpoints
    /// that snap onto an existing point reuse it, and endpoints landing on
    /// the origin reuse/create a point at (0, 0).
    fn resolve_endpoint(&self, coords: Vec2, target: SnapTarget) -> EndpointResolution {
        match target {
            SnapTarget::Point { entity } => EndpointResolution::Existing(entity),
            SnapTarget::Origin => match self.sketch.nearest_point(Vec2::ZERO, MERGE_EPS) {
                Some((id, _)) => EndpointResolution::Existing(id),
                None => EndpointResolution::New(Vec2::ZERO),
            },

            SnapTarget::Grid | SnapTarget::None => {
                match self.sketch.nearest_point(coords, MERGE_EPS) {
                    Some((id, _)) => EndpointResolution::Existing(id),
                    None => EndpointResolution::New(coords),
                }
            }
            SnapTarget::Midpoint { .. }
            | SnapTarget::ReferenceMidpoint { .. }
            | SnapTarget::ProjectedEdge { .. }
            | SnapTarget::Curve { .. } => EndpointResolution::New(coords),
            SnapTarget::Intersection { .. } => match self.sketch.nearest_point(coords, MERGE_EPS) {
                Some((id, _)) => EndpointResolution::Existing(id),
                None => EndpointResolution::New(coords),
            },
        }
    }

    fn point_has_carrier_relation(&self, point: EntityId, carrier: EntityId) -> bool {
        if self
            .sketch
            .line_endpoint_ids(carrier)
            .is_some_and(|(start, end)| start == point || end == point)
        {
            return true;
        }
        self.sketch.constraints().any(|(_, constraint)| {
            matches!(
                constraint,
                Constraint::Coincident { a, b }
                    if (*a == point && *b == carrier) || (*a == carrier && *b == point)
            )
        })
    }

    /// Attach a resolved crossing point to both carriers without duplicating
    /// structural or existing point-on-curve relations. The caller owns the
    /// surrounding snapshot and rolls back the complete line command if any
    /// relation is inconsistent.
    fn attach_crossing_relations(
        &mut self,
        point: EntityId,
        request: CurveCrossingRequest,
        created: &mut Vec<ConstraintDto>,
    ) -> Result<(), SessionError> {
        for carrier in [request.first, request.second] {
            if self.point_has_carrier_relation(point, carrier) {
                continue;
            }
            let relation = Constraint::Coincident {
                a: point,
                b: carrier,
            };
            let Some(constraint) = self.try_add_independent_auto_constraint(relation) else {
                return Err(SessionError::InvalidConstraint(
                    "Cannot keep the acquired curve crossing exact".to_string(),
                ));
            };
            created.push(constraint);
        }
        Ok(())
    }

    /// Persist an intentional origin acquisition without silently applying
    /// `Fix`. Origin coincidence removes only translation; every unrelated
    /// size/angle degree of freedom remains available to later dimensions.
    fn attach_origin_if_acquired(
        &mut self,
        entity: EntityId,
        target: SnapTarget,
    ) -> Option<ConstraintDto> {
        if target != SnapTarget::Origin {
            return None;
        }
        self.try_add_independent_auto_constraint(Constraint::OriginCoincident { entity })
    }

    /// Persist center acquisition for center-authored circles/arcs. A center
    /// snapped to a point follows that point; a center snapped to the origin
    /// keeps only its translational datum relation.
    fn attach_curve_center_if_acquired(
        &mut self,
        curve: EntityId,
        target: SnapTarget,
    ) -> Option<ConstraintDto> {
        let constraint = match target {
            SnapTarget::Origin => Constraint::OriginCoincident { entity: curve },
            SnapTarget::Point { entity: point } => Constraint::CenterCoincident { point, curve },
            SnapTarget::ProjectedEdge { edge, .. } => {
                Constraint::ReferenceOnEdge { point: curve, edge }
            }
            _ => return None,
        };
        self.try_add_independent_auto_constraint(constraint)
    }

    /// Give a curve a generated center handle bound by `CenterCoincident`.
    /// `CenterCoincident` determines the two handle coordinates from the curve
    /// center, so the handle adds no degree of freedom.
    fn attach_owned_center(&mut self, curve: EntityId, center: Vec2) -> EntityId {
        let point = self.sketch.add_generated_point(center);
        self.sketch
            .add_constraint(Constraint::CenterCoincident { point, curve });
        point
    }

    /// Bind a selectable, constrainable center point to a freshly drawn circle
    /// (issue #151). A circle's center is implicit — stored inline on
    /// `Entity::Circle` — so without this link it can be neither picked nor
    /// constrained.
    ///
    /// A free pick gets its own handle. An intentional acquisition keeps the
    /// acquired point, and a pick that lands exactly on an existing point
    /// reuses it, so no second vertex is manufactured at the same coordinate
    /// (`resolve_endpoint` does the same for line endpoints). Origin and edge
    /// acquisitions keep the curve as their datum carrier, exactly as before.
    ///
    /// The center relation is added directly rather than through the
    /// opportunistic gate: a freshly drawn circle's center is always
    /// independent, and a gate that measures rank would call it redundant
    /// whenever the handle aliases the circle's own center variables.
    fn attach_circle_center(
        &mut self,
        circle: EntityId,
        center: Vec2,
        mode: CircleMode,
        center_target: SnapTarget,
    ) -> EntityId {
        let target = if mode == CircleMode::CenterDiameter {
            center_target
        } else {
            SnapTarget::None
        };
        if !matches!(target, SnapTarget::Point { .. }) {
            self.attach_curve_center_if_acquired(circle, target);
        }
        let point = match target {
            SnapTarget::Point { entity } => entity,

            _ => match self.sketch.nearest_point(center, MERGE_EPS) {
                Some((point, _)) => point,
                None => return self.attach_owned_center(circle, center),
            },
        };
        self.sketch.add_constraint(Constraint::CenterCoincident {
            point,
            curve: circle,
        });
        point
    }

    /// Every circle that predates center handles gains one, so issue #151 is
    /// fixed for projects saved before the handle existed as well as for circles
    /// the offset tool derives. Idempotent: a circle that already has a center
    /// relation is left alone, so a user who detached a handle deliberately is
    /// not overruled until the project is reopened.
    fn attach_missing_circle_centers(&mut self) {
        let circles: Vec<(EntityId, Vec2)> = self
            .sketch
            .entities()
            .filter_map(|(id, entity)| match entity {
                Entity::Circle { center, .. } => Some((id, *center)),
                _ => None,
            })
            .collect();
        for (circle, center) in circles {
            let bound = self
                .sketch
                .constraints()
                .any(|(_, constraint)| match *constraint {
                    Constraint::CenterCoincident { curve, .. } => curve == circle,
                    _ => false,
                });
            if !bound {
                self.attach_owned_center(circle, center);
            }
        }
    }

    fn has_relation(&self, relation: &Constraint) -> bool {
        self.sketch
            .constraints()
            .any(|(_, existing)| existing.same_relation(relation))
    }

    /// Resolve a picked point-like datum for an associative curve relation.
    /// Grid/raw picks are geometry only; point/origin picks become durable.
    fn materialize_acquired_point(
        &mut self,
        position: Vec2,
        target: SnapTarget,
    ) -> Option<EntityId> {
        match target {
            SnapTarget::Point { entity } => Some(entity),
            SnapTarget::ProjectedEdge { edge, .. } => {
                let point = self.sketch.add_generated_point(position);
                self.sketch
                    .add_constraint(Constraint::ReferenceOnEdge { point, edge });
                Some(point)
            }
            SnapTarget::Origin => {
                let point = self
                    .sketch
                    .nearest_point(Vec2::ZERO, MERGE_EPS)
                    .map(|(entity, _)| entity)
                    .unwrap_or_else(|| self.sketch.add_generated_point(Vec2::ZERO));
                self.attach_origin_if_acquired(point, target);
                Some(point)
            }
            _ => None,
        }
    }

    /// Every authored arc exposes real points for selection, dragging and
    /// later line snaps. A free pick gets its own point; only an intentional
    /// acquisition shares an existing point (so Ctrl still suppresses it).
    fn attach_arc_endpoint(
        &mut self,
        arc: EntityId,
        end: crate::constraint::ArcEndpoint,
        position: Vec2,
        target: SnapTarget,
    ) -> Result<EntityId, SessionError> {
        let Some(point) = self.materialize_acquired_point(position, target) else {
            let point = self.sketch.add_generated_point(position);

            self.sketch
                .add_constraint(Constraint::ArcEndpointCoincident { point, arc, end });
            return Ok(point);
        };
        let relation = Constraint::ArcEndpointCoincident { point, arc, end };
        if !self.has_relation(&relation)
            && self.try_add_independent_auto_constraint(relation).is_none()
        {
            return Err(SessionError::InvalidConstraint(
                "Cannot preserve the acquired arc endpoint".to_string(),
            ));
        }
        Ok(point)
    }

    fn attach_curve_point_if_acquired(
        &mut self,
        curve: EntityId,
        position: Vec2,
        target: SnapTarget,
    ) -> Result<Option<EntityId>, SessionError> {
        let Some(point) = self.materialize_acquired_point(position, target) else {
            return Ok(None);
        };
        let relation = Constraint::Coincident { a: point, b: curve };
        if !self.has_relation(&relation)
            && self.try_add_independent_auto_constraint(relation).is_none()
        {
            return Err(SessionError::InvalidConstraint(
                "Cannot preserve the acquired point on the curve".to_string(),
            ));
        }
        Ok(Some(point))
    }

    /// Add tangency only when a newly created arc endpoint is already bound
    /// to a line endpoint and its authored tangent is inside the narrow
    /// inference cone. Remote or merely nearby lines are never considered.
    fn infer_arc_endpoint_tangent(
        &mut self,
        arc: EntityId,
        end: crate::constraint::ArcEndpoint,
        point: EntityId,
    ) {
        let Some(Entity::Arc {
            start_angle,
            end_angle,
            ..
        }) = self.sketch.entity(arc)
        else {
            return;
        };
        let angle = match end {
            crate::constraint::ArcEndpoint::Start => *start_angle,
            crate::constraint::ArcEndpoint::End => *end_angle,
        };
        let tangent = Vec2::new(-angle.sin(), angle.cos());
        let sine_limit = INFERENCE_ANGLE_TOL_DEG.to_radians().sin();
        let candidates = self
            .sketch
            .lines_connected_to(point)
            .into_iter()
            .filter_map(|line| {
                let (a, b) = self.sketch.resolved_line(line)?;
                let direction = b - a;
                let length = direction.length();
                if length < MIN_LINE_LENGTH_MM {
                    return None;
                }
                let parallel_error =
                    (direction.x * tangent.y - direction.y * tangent.x).abs() / length;
                (parallel_error <= sine_limit).then_some((line, parallel_error))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((line, _)) = candidates {
            self.try_add_independent_auto_constraint(Constraint::Tangent { a: line, b: arc });
        }
    }

    /// Find the best already-connected line at each endpoint that is within
    /// the normal inference cone of perpendicular to `line_id`.
    fn perpendicular_candidates(
        &self,
        line_id: EntityId,
        endpoint_ids: [EntityId; 2],
    ) -> Vec<Constraint> {
        let Some((start, end)) = self.sketch.resolved_line(line_id) else {
            return Vec::new();
        };
        let direction = end - start;
        let length = direction.length();
        if length < MIN_LINE_LENGTH_MM {
            return Vec::new();
        }
        let perpendicular_cos_limit = INFERENCE_ANGLE_TOL_DEG.to_radians().sin();
        let mut candidates = Vec::with_capacity(2);

        for endpoint_id in endpoint_ids {
            let best = self
                .sketch
                .lines_connected_to(endpoint_id)
                .into_iter()
                .filter(|candidate| *candidate != line_id)
                .filter_map(|candidate| {
                    let (a, b) = self.sketch.resolved_line(candidate)?;
                    let other = b - a;
                    let other_length = other.length();
                    if other_length < MIN_LINE_LENGTH_MM {
                        return None;
                    }
                    let absolute_cosine = direction.dot(other).abs() / (length * other_length);
                    (absolute_cosine <= perpendicular_cos_limit)
                        .then_some((candidate, absolute_cosine))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));

            if let Some((candidate, _)) = best {
                let constraint = Constraint::Perpendicular {
                    a: candidate,
                    b: line_id,
                };
                if !candidates.contains(&constraint) {
                    candidates.push(constraint);
                }
            }
        }
        candidates
    }

    /// Whether a preview segment is within the inference cone of a line
    /// already connected at either acquired endpoint. Restricting this to a
    /// shared topological point avoids surprising remote relations.
    fn preview_has_connected_perpendicular(
        &self,
        from: Vec2,
        to: Vec2,
        to_target: SnapTarget,
    ) -> bool {
        let direction = to - from;
        let length = direction.length();
        if length < MIN_LINE_LENGTH_MM {
            return false;
        }
        let mut endpoints = Vec::with_capacity(2);
        if let Some((point, _)) = self.sketch.nearest_point(from, MERGE_EPS) {
            endpoints.push(point);
        }
        if let SnapTarget::Point { entity } = to_target {
            if !endpoints.contains(&entity) {
                endpoints.push(entity);
            }
        }
        let cosine_limit = INFERENCE_ANGLE_TOL_DEG.to_radians().sin();
        endpoints.into_iter().any(|point| {
            self.sketch
                .lines_connected_to(point)
                .into_iter()
                .any(|line| {
                    let Some((a, b)) = self.sketch.resolved_line(line) else {
                        return false;
                    };
                    let other = b - a;
                    let other_length = other.length();
                    other_length >= MIN_LINE_LENGTH_MM
                        && direction.dot(other).abs() / (length * other_length) <= cosine_limit
                })
        })
    }

    /// Automatic relations are opportunistic: keep only constraints that
    /// are consistent and remove at least one independent degree of freedom.
    /// This avoids filling a closed profile with redundant relations.
    fn try_add_independent_auto_constraint(
        &mut self,
        constraint: Constraint,
    ) -> Option<ConstraintDto> {
        let already_present = self
            .sketch
            .constraints()
            .any(|(_, existing)| existing.same_relation(&constraint));
        if already_present {
            return None;
        }

        let before = self.sketch.snapshot();
        let id = self.sketch.add_constraint(constraint);
        let analysis = solver::solve(&mut self.sketch, &[]);
        let residual = solver::constraint_residual(&self.sketch, id);
        if !analysis.converged
            || residual > INCONSISTENT_EPS
            || solver::constraints_are_redundant(&self.sketch, &[id])
        {
            self.sketch.restore(before);
            return None;
        }
        self.analysis = Some(analysis);
        Some(ConstraintDto { id, constraint })
    }

    /// Preview of a segment from `from` to the raw cursor position: snapped
    /// endpoint plus the constraints that WOULD be created (D4.1).
    pub fn preview_segment(&self, from: Vec2, to_raw: Vec2, ctrl_held: bool) -> PreviewDto {
        self.snap_and_infer(from, to_raw, ctrl_held)
    }

    pub fn add_line(
        &mut self,
        from_raw: Vec2,
        to_raw: Vec2,
        ctrl_held: bool,
    ) -> Result<AddLineResult, SessionError> {
        self.add_line_impl(from_raw, to_raw, ctrl_held, None)
    }

    /// Add a line honoring locked dynamic-input fields (length/angle),
    /// auto-creating driving dimensions for typed values (D9).
    pub fn add_line_locked(
        &mut self,
        request: &LockedSegmentRequest,
    ) -> Result<AddLineResult, SessionError> {
        let length_mm = self.positive_input(request.length_text.as_deref(), request.length_mm)?;
        let angle_deg = match &request.angle_text {
            Some(t) => Some(self.eval_text(t)?),
            None => request.angle_deg,
        };
        let locks = LockedInput {
            length_mm,
            angle_deg,
            length_text: request.length_text.clone(),
            angle_text: request.angle_text.clone(),
            tracking: request.tracking,
            intersection: request.intersection,
            from_crossing: request.from_crossing,
            to_crossing: request.to_crossing,
        };
        self.add_line_impl(
            request.from,
            request.to_hint,
            request.ctrl_held,
            Some(locks),
        )
    }

    fn add_line_impl(
        &mut self,
        from_raw: Vec2,
        to_raw: Vec2,
        ctrl_held: bool,
        locks: Option<LockedInput>,
    ) -> Result<AddLineResult, SessionError> {
        let (from_coords, from_target) =
            if let Some(request) = locks.as_ref().and_then(|locks| locks.from_crossing) {
                let point = self
                    .curve_crossing_point(request, from_raw)
                    .ok_or_else(|| {
                        SessionError::InvalidConstraint(
                            "The acquired line-start crossing no longer exists".to_string(),
                        )
                    })?;
                (
                    point,
                    SnapTarget::Intersection {
                        first: request.first,
                        second: request.second,
                    },
                )
            } else {
                self.snap_line_flow(from_raw, ctrl_held)
            };
        let preview = match &locks {
            Some(locks) => self.preview_segment_locked(
                from_coords,
                (locks.length_mm, locks.angle_deg),
                to_raw,
                ctrl_held,
                (
                    locks.tracking,
                    locks.intersection,
                    locks.from_crossing,
                    locks.to_crossing,
                ),
            ),
            None => self.snap_and_infer(from_coords, to_raw, ctrl_held),
        };

        let start = self.resolve_endpoint(from_coords, from_target);
        let end = match preview.snap {
            SnapTarget::Point { entity } => EndpointResolution::Existing(entity),
            SnapTarget::Origin => self.resolve_endpoint(preview.snapped_to, preview.snap),
            SnapTarget::Grid
            | SnapTarget::None
            | SnapTarget::Midpoint { .. }
            | SnapTarget::ReferenceMidpoint { .. }
            | SnapTarget::ProjectedEdge { .. }
            | SnapTarget::Curve { .. } => EndpointResolution::New(preview.snapped_to),
            SnapTarget::Intersection { .. } => {
                self.resolve_endpoint(preview.snapped_to, preview.snap)
            }
        };

        let start_coords = match start {
            EndpointResolution::Existing(id) => self
                .sketch
                .point_position(id)
                .ok_or(SessionError::EntityNotFound(id))?,
            EndpointResolution::New(p) => p,
        };
        let end_coords = match end {
            EndpointResolution::Existing(id) => self
                .sketch
                .point_position(id)
                .ok_or(SessionError::EntityNotFound(id))?,
            EndpointResolution::New(p) => p,
        };
        if let (EndpointResolution::Existing(a), EndpointResolution::Existing(b)) = (&start, &end) {
            if a == b {
                return Err(SessionError::DegenerateSegment);
            }
        }
        if start_coords.distance(end_coords) < MIN_LINE_LENGTH_MM {
            return Err(SessionError::DegenerateSegment);
        }

        let before = self.sketch.snapshot();
        let start_point_id = match start {
            EndpointResolution::Existing(id) => id,
            EndpointResolution::New(p) => self.sketch.add_generated_point(p),
        };
        let end_point_id = match end {
            EndpointResolution::Existing(id) => id,
            EndpointResolution::New(p) => self.sketch.add_generated_point(p),
        };
        let line_id = self
            .sketch
            .add_entity(Entity::line(start_point_id, end_point_id));

        let mut created = Vec::new();
        let perpendicular_created = if ctrl_held {
            false
        } else {
            let candidates = self.perpendicular_candidates(line_id, [start_point_id, end_point_id]);
            let mut accepted = false;
            for constraint in candidates {
                if let Some(created_constraint) =
                    self.try_add_independent_auto_constraint(constraint)
                {
                    created.push(created_constraint);
                    accepted = true;
                }
            }
            accepted
        };
        for inference in &preview.inferences {
            let constraint = match inference {
                Inference::Horizontal if !perpendicular_created => {
                    Some(Constraint::Horizontal { entity: line_id })
                }
                Inference::Vertical if !perpendicular_created => {
                    Some(Constraint::Vertical { entity: line_id })
                }
                Inference::Horizontal | Inference::Vertical => None,
                Inference::Perpendicular => None,

                Inference::Coincident => None,
            };
            if let Some(c) = constraint {
                let id = self.sketch.add_constraint(c);
                created.push(ConstraintDto { id, constraint: c });
            }
        }
        for (point_id, target) in [(start_point_id, from_target), (end_point_id, preview.snap)] {
            if let Some(constraint) = self.attach_origin_if_acquired(point_id, target) {
                created.push(constraint);
            }
        }

        for (target, point_id) in [(from_target, start_point_id), (preview.snap, end_point_id)] {
            let SnapTarget::Intersection { first, second } = target else {
                continue;
            };
            if let Err(error) = self.attach_crossing_relations(
                point_id,
                CurveCrossingRequest { first, second },
                &mut created,
            ) {
                self.sketch.restore(before);
                self.recompute();
                return Err(error);
            }
        }

        if let SnapTarget::Curve { entity: carrier } = preview.snap {
            let constraint = Constraint::Coincident {
                a: end_point_id,
                b: carrier,
            };
            let Some(created_constraint) = self.try_add_independent_auto_constraint(constraint)
            else {
                self.sketch.restore(before);
                self.recompute();
                return Err(SessionError::InvalidConstraint(
                    "Cannot attach the line endpoint to the acquired curve".to_string(),
                ));
            };
            created.push(created_constraint);
        }

        for (point_id, target) in [(start_point_id, from_target), (end_point_id, preview.snap)] {
            if let Some(c) = self.midpoint_constraint_for_target(point_id, target) {
                let id = self.sketch.add_constraint(c);
                created.push(ConstraintDto { id, constraint: c });
            }
        }

        if let Some(locks) = &locks {
            if let Some(text) = locks.length_text.as_deref() {
                self.auto_dim_line_length(line_id, text);
            } else if let Some(v) = locks.length_mm {
                self.auto_dim_line_length(line_id, &format_number(v));
            }
            if let Some(text) = locks.angle_text.as_deref() {
                self.auto_dim_line_angle(line_id, text);
            } else if let Some(v) = locks.angle_deg {
                self.auto_dim_line_angle(line_id, &format_number(v));
            }
        }

        self.recompute();
        self.push_command(before);
        Ok(AddLineResult {
            entity_id: line_id,
            start_point_id,
            end_point_id,
            created_constraints: created,
            sketch: self.dto(),
        })
    }

    /// A single standalone point (Point tool), snapped.
    pub fn add_point(&mut self, raw: Vec2) -> Result<ToolResult, SessionError> {
        self.add_point_on_selective(raw, None, false)
    }

    /// A Point-tool placement with an optional acquired carrier curve.
    /// Keeping creation and Coincident in one engine operation makes Undo
    /// atomic and prevents a visually-on-curve point from remaining free.
    pub fn add_point_on(
        &mut self,
        raw: Vec2,
        coincident_with: Option<EntityId>,
    ) -> Result<ToolResult, SessionError> {
        self.add_point_on_selective(raw, coincident_with, false)
    }

    pub fn add_point_on_selective(
        &mut self,
        raw: Vec2,
        coincident_with: Option<EntityId>,
        ctrl_held: bool,
    ) -> Result<ToolResult, SessionError> {
        if let Some(carrier) = coincident_with.filter(|_| !ctrl_held) {
            let position = self.point_projected_to_curve(carrier, raw)?;
            if let Some((id, _)) = self.sketch.nearest_point(position, MERGE_EPS) {
                let before = self.sketch.snapshot();
                if self.sketch.retain_point(id) {
                    self.push_command(before);
                }
                return Ok(ToolResult {
                    entities: vec![id],
                    sketch: self.dto(),
                });
            }

            let before = self.sketch.snapshot();
            let id = self.sketch.add_entity(Entity::Point { position });
            let constraint = Constraint::Coincident { a: id, b: carrier };
            self.validate_constraint(&constraint)?;
            let cid = self.sketch.add_constraint(constraint);
            let analysis = solver::solve(&mut self.sketch, &[]);
            let residual = solver::constraint_residual(&self.sketch, cid);
            if !analysis.converged || residual > INCONSISTENT_EPS {
                self.sketch.restore(before);
                self.recompute();
                return Err(SessionError::InvalidConstraint(
                    "Cannot place a constrained point on the selected curve".to_string(),
                ));
            }
            self.analysis = Some(analysis);
            self.push_command(before);
            return Ok(ToolResult {
                entities: vec![id],
                sketch: self.dto(),
            });
        }

        let (coords, target) = self.snap_creation_carrier_only(raw, ctrl_held);
        let resolution = self.resolve_endpoint(coords, target);
        if let EndpointResolution::Existing(id) = resolution {
            let before = self.sketch.snapshot();
            let adopted = self.sketch.retain_point(id);
            if self.attach_origin_if_acquired(id, target).is_some() || adopted {
                self.recompute();
                self.push_command(before);
            }
            return Ok(ToolResult {
                entities: vec![id],
                sketch: self.dto(),
            });
        }
        let before = self.sketch.snapshot();
        let EndpointResolution::New(p) = resolution else {
            unreachable!()
        };
        let id = self.sketch.add_entity(Entity::Point { position: p });
        self.attach_origin_if_acquired(id, target);
        if let Some(relation) = self.midpoint_constraint_for_target(id, target) {
            self.try_add_independent_auto_constraint(relation);
        }
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities: vec![id],
            sketch: self.dto(),
        })
    }

    fn point_projected_to_curve(&self, carrier: EntityId, raw: Vec2) -> Result<Vec2, SessionError> {
        match self.sketch.entity(carrier) {
            Some(Entity::Line { .. }) => {
                let (start, end) = self
                    .sketch
                    .resolved_line(carrier)
                    .ok_or(SessionError::EntityNotFound(carrier))?;
                let delta = end - start;
                let length_squared = delta.dot(delta);
                if length_squared <= MERGE_EPS * MERGE_EPS {
                    return Err(SessionError::DegenerateSegment);
                }

                let t = (raw - start).dot(delta) / length_squared;
                Ok(start + delta * t)
            }
            Some(Entity::Circle { center, radius }) => {
                let delta = raw - *center;
                let length = delta.length();
                let direction = if length <= MERGE_EPS {
                    Vec2::new(1.0, 0.0)
                } else {
                    delta * (1.0 / length)
                };
                Ok(*center + direction * *radius)
            }
            Some(Entity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            }) => {
                let delta = raw - *center;
                let angle = if delta.length() <= MERGE_EPS {
                    *start_angle
                } else {
                    delta.y.atan2(delta.x)
                };
                let sweep = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
                let clamped_angle = if sweep(*start_angle, angle) <= sweep(*start_angle, *end_angle)
                {
                    angle
                } else {
                    let start_point =
                        *center + Vec2::new(start_angle.cos(), start_angle.sin()) * *radius;
                    let end_point = *center + Vec2::new(end_angle.cos(), end_angle.sin()) * *radius;
                    if raw.distance(start_point) <= raw.distance(end_point) {
                        *start_angle
                    } else {
                        *end_angle
                    }
                };
                Ok(*center + Vec2::new(clamped_angle.cos(), clamped_angle.sin()) * *radius)
            }
            Some(_) => Err(SessionError::InvalidConstraint(
                "Point-on-curve placement supports lines, circles, and arcs".to_string(),
            )),
            None => Err(SessionError::EntityNotFound(carrier)),
        }
    }

    /// Midpoint Line: a line whose midpoint is `mid_raw`; `end_raw` is one
    /// endpoint; the other mirrors through the midpoint.
    pub fn add_line_midpoint(
        &mut self,
        mid_raw: Vec2,
        end_raw: Vec2,
        ctrl_held: bool,
    ) -> Result<ToolResult, SessionError> {
        let (mid, mid_target) = self.snap_line_flow(mid_raw, ctrl_held);
        let preview = self.snap_and_infer(mid, end_raw, ctrl_held);
        let end = preview.snapped_to;
        let other = mid * 2.0 - end;
        if end.distance(other) < MIN_LINE_LENGTH_MM {
            return Err(SessionError::DegenerateSegment);
        }

        let mid_resolution = self.resolve_endpoint(mid, mid_target);
        let end_resolution = self.resolve_endpoint(end, preview.snap);

        let other_resolution = self
            .sketch
            .nearest_point(other, MERGE_EPS)
            .map(|(id, _)| EndpointResolution::Existing(id))
            .unwrap_or(EndpointResolution::New(other));

        let coords = |resolution: &EndpointResolution| match resolution {
            EndpointResolution::Existing(id) => self.sketch.point_position(*id),
            EndpointResolution::New(point) => Some(*point),
        };
        let (Some(mid_coords), Some(end_coords), Some(other_coords)) = (
            coords(&mid_resolution),
            coords(&end_resolution),
            coords(&other_resolution),
        ) else {
            return Err(SessionError::DegenerateSegment);
        };
        if end_coords.distance(other_coords) < MIN_LINE_LENGTH_MM
            || mid_coords.distance(end_coords) < MIN_LINE_LENGTH_MM
        {
            return Err(SessionError::DegenerateSegment);
        }

        let before = self.sketch.snapshot();
        let mut materialize = |resolution: EndpointResolution| match resolution {
            EndpointResolution::Existing(id) => id,
            EndpointResolution::New(position) => self.sketch.add_generated_point(position),
        };
        let mid_id = materialize(mid_resolution);
        let a_id = materialize(other_resolution);
        let b_id = materialize(end_resolution);
        if a_id == b_id || mid_id == a_id || mid_id == b_id {
            self.sketch.restore(before);
            return Err(SessionError::DegenerateSegment);
        }
        let line_id = self.sketch.add_entity(Entity::line(a_id, b_id));
        self.sketch.add_constraint(Constraint::Midpoint {
            a: mid_id,
            b: line_id,
        });
        let perpendicular_created = if ctrl_held {
            false
        } else {
            let mut accepted = false;
            for constraint in self.perpendicular_candidates(line_id, [a_id, b_id]) {
                accepted |= self
                    .try_add_independent_auto_constraint(constraint)
                    .is_some();
            }
            accepted
        };
        for inference in preview.inferences {
            match inference {
                Inference::Horizontal if !perpendicular_created => {
                    self.sketch
                        .add_constraint(Constraint::Horizontal { entity: line_id });
                }
                Inference::Vertical if !perpendicular_created => {
                    self.sketch
                        .add_constraint(Constraint::Vertical { entity: line_id });
                }
                Inference::Horizontal | Inference::Vertical | Inference::Perpendicular => {}
                Inference::Coincident => {}
            }
        }
        if let Some(constraint) = self.midpoint_constraint_for_target(mid_id, mid_target) {
            self.sketch.add_constraint(constraint);
        }
        if let Some(constraint) = self.midpoint_constraint_for_target(b_id, preview.snap) {
            self.sketch.add_constraint(constraint);
        }
        self.attach_origin_if_acquired(mid_id, mid_target);
        self.attach_origin_if_acquired(b_id, preview.snap);
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities: vec![mid_id, a_id, b_id, line_id],
            sketch: self.dto(),
        })
    }

    /// Rectangle (2-Point or Center), axis-aligned to the sketch u/v axes:
    /// 4 lines with H/V constraints and structural coincident corners.
    pub fn add_rectangle(
        &mut self,
        mode: RectangleMode,
        p1: Vec2,
        p2: Vec2,
    ) -> Result<ToolResult, SessionError> {
        self.add_rectangle_selective(mode, p1, p2, false)
    }

    pub fn add_rectangle_selective(
        &mut self,
        mode: RectangleMode,
        p1: Vec2,
        p2: Vec2,
        ctrl_held: bool,
    ) -> Result<ToolResult, SessionError> {
        let (a, a_target) = self.snap_creation(p1, ctrl_held);
        let (b, b_target) = self.snap_creation(p2, ctrl_held);
        self.build_rectangle(mode, a, b, [(a, a_target), (b, b_target)], !ctrl_held)
    }

    /// Rectangle honoring locked width/height dynamic-input fields.
    /// Locks constrain only their own axis (D9): free axes still grid-snap,
    /// and typed values auto-create driving dimensions (one undo command).
    pub fn add_rectangle_locked(
        &mut self,
        request: &LockedRectangleRequest,
    ) -> Result<ToolResult, SessionError> {
        let mode = request.mode;
        let creation::ResolvedRectangle {
            anchor,
            anchor_target,
            corner,
            corner_target: hint_target,
            width: width_mm,
            height: height_mm,
        } = self.resolve_rectangle(request)?;

        let before = self.sketch.snapshot();
        let entities = self.create_rectangle(mode, anchor, corner, !request.ctrl_held)?;
        self.attach_rectangle_acquisitions(
            &entities,
            [(anchor, anchor_target), (corner, hint_target)],
        );

        let (bl, br, tl) = (entities[0], entities[1], entities[3]);
        let w_text = request
            .width_text
            .clone()
            .or_else(|| width_mm.map(format_number));
        let h_text = request
            .height_text
            .clone()
            .or_else(|| height_mm.map(format_number));
        self.auto_dim_rect(bl, br, tl, w_text.as_deref(), h_text.as_deref());
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities,
            sketch: self.dto(),
        })
    }

    /// Native preview uses the same locked-axis resolution as commit.
    pub fn preview_rectangle_locked(
        &self,
        request: &LockedRectangleRequest,
    ) -> Result<[Vec2; 2], SessionError> {
        let resolved = self.resolve_rectangle(request)?;
        Ok([resolved.anchor, resolved.corner])
    }

    fn build_rectangle(
        &mut self,
        mode: RectangleMode,
        p1: Vec2,
        p2: Vec2,
        acquisitions: [(Vec2, SnapTarget); 2],
        allow_merge: bool,
    ) -> Result<ToolResult, SessionError> {
        let before = self.sketch.snapshot();
        let entities = self.create_rectangle(mode, p1, p2, allow_merge)?;
        self.attach_rectangle_acquisitions(&entities, acquisitions);
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities,
            sketch: self.dto(),
        })
    }

    fn attach_rectangle_acquisitions(
        &mut self,
        entities: &[EntityId],
        acquisitions: [(Vec2, SnapTarget); 2],
    ) {
        for (position, target) in acquisitions {
            if let Some(point) = entities.iter().copied().find(|id| {
                self.sketch
                    .point_position(*id)
                    .is_some_and(|p| p.distance(position) <= MERGE_EPS)
            }) {
                self.attach_origin_if_acquired(point, target);
                if let Some(relation) = self.midpoint_constraint_for_target(point, target) {
                    self.try_add_independent_auto_constraint(relation);
                }
            }
        }
    }

    /// Rectangle mutation only (shared by plain and locked/dimensioned
    /// creation): 4 corner points + 4 H/V-constrained lines, returned as
    /// [points…, lines…].
    ///
    /// A center rectangle also owns a real, selectable center point, appended
    /// after the corners and lines so callers that index the corners as
    /// `entities[0..=3]` stay valid. Binding that point to a diagonal's
    /// midpoint is what keeps the rectangle symmetric about its center: a
    /// corner drag moves the opposite corner instead of leaving the far side
    /// behind (issue #151).
    fn create_rectangle(
        &mut self,
        mode: RectangleMode,
        p1: Vec2,
        p2: Vec2,
        allow_merge: bool,
    ) -> Result<Vec<EntityId>, SessionError> {
        let corners = Self::rectangle_corners(mode, p1, p2)?;
        let mut point_ids = Vec::with_capacity(4);
        for c in corners {
            let existing = (self.point_snap && allow_merge)
                .then(|| self.sketch.nearest_point(c, MERGE_EPS))
                .flatten()
                .map(|(id, _)| id);
            let point_id = existing.unwrap_or_else(|| self.sketch.add_generated_point(c));
            point_ids.push(point_id);
        }
        let mut line_ids = Vec::with_capacity(4);
        for i in 0..4 {
            line_ids.push(
                self.sketch
                    .add_entity(Entity::line(point_ids[i], point_ids[(i + 1) % 4])),
            );
        }

        self.sketch.add_constraint(Constraint::Horizontal {
            entity: line_ids[0],
        });
        self.sketch.add_constraint(Constraint::Horizontal {
            entity: line_ids[2],
        });
        self.sketch.add_constraint(Constraint::Vertical {
            entity: line_ids[1],
        });
        self.sketch.add_constraint(Constraint::Vertical {
            entity: line_ids[3],
        });
        let mut entities = point_ids.clone();
        entities.extend(line_ids);
        if mode == RectangleMode::Center {
            let position = (corners[0] + corners[2]) * 0.5;
            let existing = (self.point_snap && allow_merge)
                .then(|| self.sketch.nearest_point(position, MERGE_EPS))
                .flatten()
                .map(|(id, _)| id)
                .filter(|id| !point_ids.contains(id));
            let center = existing.unwrap_or_else(|| self.sketch.add_generated_point(position));
            self.sketch.add_constraint(Constraint::SpanMidpoint {
                point: center,
                start: point_ids[0],
                end: point_ids[2],
            });
            entities.push(center);
        }
        Ok(entities)
    }

    /// Circle (Center-Diameter or 2-Point diameter).
    pub fn add_circle(
        &mut self,
        mode: CircleMode,
        p1: Vec2,
        p2: Vec2,
    ) -> Result<ToolResult, SessionError> {
        self.add_circle_selective(mode, p1, p2, false)
    }

    pub fn add_circle_selective(
        &mut self,
        mode: CircleMode,
        p1: Vec2,
        p2: Vec2,
        ctrl_held: bool,
    ) -> Result<ToolResult, SessionError> {
        let (a, a_target) = self.snap_creation(p1, ctrl_held);
        let (b, _) = self.snap_creation(p2, ctrl_held);
        self.build_circle(mode, a, b, a_target)
    }

    /// Circle honoring a locked diameter field (typed value auto-creates a
    /// Diameter dimension, D9). `anchor` is the center (Center-Diameter) or
    /// first diameter endpoint (2-Point); `edge_hint` supplies the
    /// radius/direction when unlocked.
    pub fn add_circle_locked(
        &mut self,
        request: &LockedCircleRequest,
    ) -> Result<ToolResult, SessionError> {
        let mode = request.mode;
        let creation::ResolvedCircle {
            anchor,
            anchor_target,
            edge: second,
            diameter: diameter_mm,
            ..
        } = self.resolve_circle(request)?;

        let before = self.sketch.snapshot();
        let id = self.create_circle(mode, anchor, second, anchor_target)?;
        let d_text = request
            .diameter_text
            .clone()
            .or_else(|| diameter_mm.map(format_number));
        if let Some(text) = d_text.as_deref() {
            self.auto_dim_circle(id, text);
        }
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities: vec![id],
            sketch: self.dto(),
        })
    }

    /// Native preview uses the same locked-diameter resolution as commit.
    pub fn preview_circle_locked(
        &self,
        request: &LockedCircleRequest,
    ) -> Result<[Vec2; 2], SessionError> {
        let resolved = self.resolve_circle(request)?;
        Ok([resolved.anchor, resolved.edge])
    }

    fn build_circle(
        &mut self,
        mode: CircleMode,
        p1: Vec2,
        p2: Vec2,
        center_target: SnapTarget,
    ) -> Result<ToolResult, SessionError> {
        let before = self.sketch.snapshot();
        let id = self.create_circle(mode, p1, p2, center_target)?;
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities: vec![id],
            sketch: self.dto(),
        })
    }

    /// Circle mutation only (shared by plain and locked/dimensioned creation),
    /// including seating the selectable center handle so every circle tool goes
    /// through one path.
    fn create_circle(
        &mut self,
        mode: CircleMode,
        p1: Vec2,
        p2: Vec2,
        center_target: SnapTarget,
    ) -> Result<EntityId, SessionError> {
        let (center, radius) = match mode {
            CircleMode::CenterDiameter => (p1, p1.distance(p2)),
            CircleMode::TwoPoint => (((p1 + p2) * 0.5), p1.distance(p2) / 2.0),
        };
        if radius < MIN_LINE_LENGTH_MM {
            return Err(SessionError::DegenerateSegment);
        }
        let id = self.sketch.add_entity(Entity::Circle { center, radius });
        self.attach_circle_center(id, center, mode, center_target);
        Ok(id)
    }

    /// Slot (M1 follow-up): a capsule of 2 parallel
    /// lines + 2 semicircular end-cap arcs, tangent by construction
    /// (geomops::slot), with Tangent/Parallel/Equal constraints and a
    /// best-effort Ø width dimension on typed input (D9). One undo command.
    pub fn add_slot(&mut self, request: &SlotRequest) -> Result<ToolResult, SessionError> {
        let (cap, _, _, width) = self.resolve_slot(request)?;
        let width_locked = request.width_mm;

        let before = self.sketch.snapshot();
        let pa1 = self.sketch.add_generated_point(cap.line1.a);
        let pa2 = self.sketch.add_generated_point(cap.line1.b);
        let pb1 = self.sketch.add_generated_point(cap.line2.a);
        let pb2 = self.sketch.add_generated_point(cap.line2.b);
        let line1 = self.sketch.add_entity(Entity::line(pa1, pa2));
        let line2 = self.sketch.add_entity(Entity::line(pb1, pb2));
        let arc1 = self.sketch.add_entity(Entity::Arc {
            center: cap.arc1.center,
            radius: cap.arc1.radius,
            start_angle: cap.arc1.start_angle,
            end_angle: cap.arc1.end_angle,
        });
        let arc2 = self.sketch.add_entity(Entity::Arc {
            center: cap.arc2.center,
            radius: cap.arc2.radius,
            start_angle: cap.arc2.start_angle,
            end_angle: cap.arc2.end_angle,
        });
        self.sketch
            .add_constraint(Constraint::Tangent { a: line1, b: arc1 });
        self.sketch
            .add_constraint(Constraint::Tangent { a: line2, b: arc1 });
        self.sketch
            .add_constraint(Constraint::Tangent { a: line1, b: arc2 });
        self.sketch
            .add_constraint(Constraint::Tangent { a: line2, b: arc2 });
        self.sketch
            .add_constraint(Constraint::Parallel { a: line1, b: line2 });
        self.sketch
            .add_constraint(Constraint::Equal { a: arc1, b: arc2 });

        use crate::constraint::ArcEndpoint::{End as AEnd, Start as AStart};
        self.sketch
            .add_constraint(Constraint::ArcEndpointCoincident {
                point: pa1,
                arc: arc1,
                end: AStart,
            });
        self.sketch
            .add_constraint(Constraint::ArcEndpointCoincident {
                point: pb1,
                arc: arc1,
                end: AEnd,
            });
        self.sketch
            .add_constraint(Constraint::ArcEndpointCoincident {
                point: pb2,
                arc: arc2,
                end: AStart,
            });
        self.sketch
            .add_constraint(Constraint::ArcEndpointCoincident {
                point: pa2,
                arc: arc2,
                end: AEnd,
            });

        let w_text = request
            .width_text
            .clone()
            .or_else(|| width_locked.map(format_number));
        if let Some(text) = w_text.as_deref() {
            if let Ok(param) =
                self.param_from_text(crate::params::ParamKind::Length, Some(text), width)
            {
                let pos =
                    cap.arc1.center + Vec2::new(cap.arc1.radius + 12.0, cap.arc1.radius + 12.0);
                let _ = self.add_constraint_bound(
                    Constraint::Diameter {
                        entity: arc1,
                        value: width,
                    },
                    param,
                    pos,
                    false,
                );
            }
        }
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities: vec![pa1, pa2, pb1, pb2, line1, line2, arc1, arc2],
            sketch: self.dto(),
        })
    }

    /// Fit-point spline (M1 follow-up): centripetal Catmull-Rom through the
    /// fit points (geomops::spline). Self-contained entity — no shared
    /// points, no constraints in v1; one undo record. Consecutive duplicate
    /// picks are dropped (zero-length spans).
    pub fn add_spline(&mut self, request: &SplineRequest) -> Result<ToolResult, SessionError> {
        let mut points: Vec<Vec2> = Vec::with_capacity(request.points.len());
        for &p in &request.points {
            let (q, _) = self.snap(p);
            if points
                .last()
                .is_none_or(|last: &Vec2| last.distance(q) > MIN_LINE_LENGTH_MM)
            {
                points.push(q);
            }
        }
        if points.len() < 2 {
            return Err(SessionError::DegenerateSegment);
        }
        let before = self.sketch.snapshot();
        let id = self.sketch.add_entity(Entity::Spline { points });
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities: vec![id],
            sketch: self.dto(),
        })
    }

    /// Magnetic 1D snap of a free axis component.
    fn snap_1d_with_status(&self, v: f64) -> Option<f64> {
        if !self.grid_snap {
            return None;
        }
        let snapped = (v / self.grid_step).round() * self.grid_step;
        ((snapped - v).abs() <= self.grid_capture_distance()).then_some(snapped)
    }

    /// 3-Point Arc: circumscribed circle through p1 (start), p2 (on-arc),
    /// p3 (end); the CCW sweep from start to end contains p2.
    pub fn add_arc_3pt(
        &mut self,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    ) -> Result<ToolResult, SessionError> {
        self.add_arc_3pt_selective(p1, p2, p3, false)
    }

    pub fn add_arc_3pt_selective(
        &mut self,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
        ctrl_held: bool,
    ) -> Result<ToolResult, SessionError> {
        let creation::ResolvedArc {
            center,
            radius,
            start_angle,
            end_angle,
            start_pick,
            end_pick,
            mid_pick,
            ..
        } = self.resolve_three_point_arc(&crate::dto::Arc3PointRequest {
            p1,
            p2,
            p3,
            ctrl_held,
        })?;
        let (p2, p2_target) = mid_pick.expect("three-point arc has a middle pick");
        let before = self.sketch.snapshot();
        let id = self.sketch.add_entity(Entity::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        });
        let start_point = match self.attach_arc_endpoint(
            id,
            crate::constraint::ArcEndpoint::Start,
            start_pick.0,
            start_pick.1,
        ) {
            Ok(point) => point,
            Err(error) => {
                self.sketch.restore(before);
                self.recompute();
                return Err(error);
            }
        };
        let end_point = match self.attach_arc_endpoint(
            id,
            crate::constraint::ArcEndpoint::End,
            end_pick.0,
            end_pick.1,
        ) {
            Ok(point) => point,
            Err(error) => {
                self.sketch.restore(before);
                self.recompute();
                return Err(error);
            }
        };
        if let Err(error) = self.attach_curve_point_if_acquired(id, p2, p2_target) {
            self.sketch.restore(before);
            self.recompute();
            return Err(error);
        }
        if !ctrl_held {
            self.infer_arc_endpoint_tangent(id, crate::constraint::ArcEndpoint::Start, start_point);
            self.infer_arc_endpoint_tangent(id, crate::constraint::ArcEndpoint::End, end_point);
        }
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities: vec![id],
            sketch: self.dto(),
        })
    }

    /// Center Arc: center, start point (defines radius + start angle), and
    /// a sweep point defining the end angle (CCW sweep).
    pub fn add_arc_center(
        &mut self,
        center: Vec2,
        start: Vec2,
        sweep: Vec2,
    ) -> Result<ToolResult, SessionError> {
        self.add_arc_center_selective(center, start, sweep, false)
    }

    pub fn add_arc_center_selective(
        &mut self,
        center: Vec2,
        start: Vec2,
        sweep: Vec2,
        ctrl_held: bool,
    ) -> Result<ToolResult, SessionError> {
        self.build_center_arc((center, start, sweep), ctrl_held, None, None, None, None)
    }

    /// Center Arc honoring a locked radius field (typed value auto-creates a
    /// Radius dimension, D9). While the radius is locked the cursor supplies
    /// each pick's direction only, so the second and third clicks still aim
    /// the arc. `sweep_rad` is the pointer's signed travel from the start pick;
    /// see [`Self::build_center_arc`] for how a clockwise sweep is stored.
    pub fn add_arc_center_locked(
        &mut self,
        (center, start, sweep): (Vec2, Vec2, Vec2),
        ctrl_held: bool,
        radius_mm: Option<f64>,
        radius_text: Option<&str>,
        angle_text: Option<&str>,
        sweep_rad: Option<f64>,
    ) -> Result<ToolResult, SessionError> {
        let radius = match radius_text {
            Some(text) => Some(self.eval_text(text)?),
            None => radius_mm,
        };
        self.build_center_arc(
            (center, start, sweep),
            ctrl_held,
            radius,
            radius_text,
            angle_text,
            sweep_rad,
        )
    }

    /// Place a locked-radius pick on the authored radius in the cursor's
    /// direction. A vertex already at that radius wins when it is under the
    /// cursor, so a radius lock still composes with point acquisition.
    fn radius_locked_point(
        &self,
        center: Vec2,
        radius: f64,
        hint: Vec2,
        ctrl_held: bool,
    ) -> (Vec2, SnapTarget) {
        let (acquired, target) = self.snap_creation(hint, ctrl_held);
        if (center.distance(acquired) - radius).abs() <= MERGE_EPS {
            return (acquired, target);
        }
        let direction = hint - center;
        let length = direction.length();
        let unit = if length < MERGE_EPS {
            Vec2::new(1.0, 0.0)
        } else {
            direction * (1.0 / length)
        };
        let edge = center + unit * radius;
        (edge, SnapTarget::None)
    }

    fn build_center_arc(
        &mut self,
        (center, start, sweep): (Vec2, Vec2, Vec2),
        ctrl_held: bool,
        locked_radius: Option<f64>,
        radius_text: Option<&str>,
        angle_text: Option<&str>,
        sweep_rad: Option<f64>,
    ) -> Result<ToolResult, SessionError> {
        let creation::ResolvedArc {
            center,
            radius,
            start_angle,
            end_angle,
            start_pick,
            end_pick,
            center_target,
            lock,
            ..
        } = self.resolve_center_arc(&crate::dto::ArcCenterRequest {
            center,
            start,
            sweep,
            ctrl_held,
            radius_mm: locked_radius,
            radius_text: radius_text.map(str::to_owned),
            angle_text: angle_text.map(str::to_owned),
            sweep_rad,
        })?;
        let before = self.sketch.snapshot();
        let id = self.sketch.add_entity(Entity::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        });
        self.attach_curve_center_if_acquired(id, center_target);
        let (start_position, stored_start_target) = start_pick;
        let (end_position, stored_end_target) = end_pick;
        let start_point = match self.attach_arc_endpoint(
            id,
            crate::constraint::ArcEndpoint::Start,
            start_position,
            stored_start_target,
        ) {
            Ok(point) => point,
            Err(error) => {
                self.sketch.restore(before);
                self.recompute();
                return Err(error);
            }
        };
        let end_point = match self.attach_arc_endpoint(
            id,
            crate::constraint::ArcEndpoint::End,
            end_position,
            stored_end_target,
        ) {
            Ok(point) => point,
            Err(error) => {
                self.sketch.restore(before);
                self.recompute();
                return Err(error);
            }
        };
        if !ctrl_held {
            self.infer_arc_endpoint_tangent(id, crate::constraint::ArcEndpoint::Start, start_point);
            self.infer_arc_endpoint_tangent(id, crate::constraint::ArcEndpoint::End, end_point);
        }

        let dim_text = radius_text
            .map(str::to_owned)
            .or_else(|| lock.map(format_number));
        if let Some(text) = dim_text.as_deref() {
            self.auto_dim_arc_radius(id, text);
        }

        if let Some(text) = angle_text.map(str::to_owned) {
            self.auto_dim_arc_angle(id, &text);
        }
        self.recompute();
        self.push_command(before);
        Ok(ToolResult {
            entities: vec![id],
            sketch: self.dto(),
        })
    }

    /// The center a corner drag must not disturb: a generated span-midpoint
    /// center anchored by this corner. A center rectangle resizes *about* its
    /// center, so the dragged corner moves and the opposite corner mirrors it,
    /// rather than the whole shape sliding and the center absorbing the change.
    ///
    /// A rectangle's diagonal names two opposite corners, but the user can grab
    /// any of the four. The other two are the ones a single line away from a
    /// named corner, so the center is pinned for those as well.
    fn span_center_anchored_by(&self, corner: EntityId) -> Option<EntityId> {
        let neighbours: Vec<EntityId> = self
            .sketch
            .lines_connected_to(corner)
            .into_iter()
            .filter_map(|line| self.sketch.line_endpoint_ids(line))
            .flat_map(|(a, b)| [a, b])
            .filter(|point| *point != corner)
            .collect();
        self.sketch
            .constraints()
            .filter_map(|(_, constraint)| match *constraint {
                Constraint::SpanMidpoint { point, start, end } => Some((point, start, end)),
                _ => None,
            })
            .find(|(point, start, end)| {
                (corner == *start
                    || corner == *end
                    || neighbours.contains(start)
                    || neighbours.contains(end))
                    && self.is_owned_span_center(*point)
            })
            .map(|(point, _, _)| point)
    }

    /// A generated rectangle center remains a resize anchor when circles also
    /// use it. Pinning its existing position keeps those circles stationary;
    /// the failed-pin fallback still handles dimensioned translation cases.
    fn is_owned_span_center(&self, point: EntityId) -> bool {
        self.sketch.is_generated_point(point)
            && !self.sketch.is_referenced_by_entity(point)
            && self.sketch.relations_pointing_at(point).all(|constraint| {
                matches!(constraint,
                    Constraint::SpanMidpoint { point: center, .. }
                    | Constraint::CenterCoincident { point: center, .. } if *center == point)
            })
    }

    /// Rubber-band drag of a point via the solver: the dragged point is
    /// pinned to the cursor, everything else is re-solved each call, so
    /// coincident/shared points and all constraints keep holding (D4.4).
    pub fn move_point(
        &mut self,
        request: MovePointRequest,
    ) -> Result<MovePointResult, SessionError> {
        let point_id = request.point_id;
        if self.sketch.entity(point_id).is_none() {
            return Err(SessionError::EntityNotFound(point_id));
        }
        let current = self
            .sketch
            .point_position(point_id)
            .ok_or(SessionError::NotAPoint(point_id))?;
        let (target, _) = self.snap_inner(request.to_raw, !request.ctrl_held, false, None);
        if request.phase == DragPhase::Single
            && target == current
            && self.pending_drag.is_none()
            && self.last_good_drag.is_none()
            && solver::analyze(&self.sketch).converged
        {
            return Ok(MovePointResult { sketch: self.dto() });
        }

        if matches!(request.phase, DragPhase::Begin | DragPhase::Single) {
            self.pending_drag = Some(self.sketch.snapshot());
            self.last_good_drag = Some(self.sketch.snapshot());
        }

        let mut pins = vec![(point_id, target)];

        if let Some(center) = self.span_center_anchored_by(point_id) {
            if let Some(position) = self.sketch.point_position(center) {
                pins.push((center, position));
            }
        }

        let before_pins = (pins.len() > 1).then(|| self.sketch.snapshot());
        let mut analysis = solver::solve(&mut self.sketch, &pins);
        if !analysis.converged {
            if let Some(before_pins) = before_pins {
                self.sketch.restore(before_pins);
                analysis = solver::solve(&mut self.sketch, &[(point_id, target)]);
            }
        }
        if analysis.converged {
            if let Some(Entity::Point { position }) = self.sketch.entity_mut(point_id) {
                *position = target;
            }
            self.analysis = Some(solver::analyze(&self.sketch));
            self.last_good_drag = Some(self.sketch.snapshot());
        } else if request.phase == DragPhase::Single {
            // A discrete failed drag has no last-good progress to commit.
            // Restore its baseline without consuming an Undo or Redo entry.
            if let Some(before) = self.pending_drag.take() {
                self.sketch.restore(before);
            }
            self.last_good_drag = None;
            self.analysis = Some(solver::analyze(&self.sketch));
            return Err(SessionError::InvalidConstraint(
                "The sketch solver could not move this point while preserving its constraints. Try a different position or review the point's constraints."
                    .into(),
            ));
        } else if let Some(good) = self.last_good_drag.take() {
            self.sketch.restore(good.clone());
            self.last_good_drag = Some(good);
            self.analysis = Some(solver::analyze(&self.sketch));
        }

        if matches!(request.phase, DragPhase::End | DragPhase::Single) {
            if let Some(before) = self.pending_drag.take() {
                self.push_command(before);
            }
            self.last_good_drag = None;
        }

        Ok(MovePointResult { sketch: self.dto() })
    }

    /// Delete an entity. Deleting a point cascades to connected lines (and
    /// their constraints). Generated handles disappear only when unused;
    /// independently authored points and shared/constrained handles remain.
    pub fn delete_entity(&mut self, id: EntityId) -> Result<DeleteEntityResult, SessionError> {
        self.delete_entities(&[id])
    }

    /// Batch delete (multi-select) as one undoable command.
    pub fn delete_entities(
        &mut self,
        ids: &[EntityId],
    ) -> Result<DeleteEntityResult, SessionError> {
        let existing: Vec<EntityId> = ids
            .iter()
            .copied()
            .filter(|id| self.sketch.entity(*id).is_some())
            .collect();
        if existing.is_empty() {
            return Err(SessionError::EntityNotFound(
                ids.first().copied().unwrap_or(EntityId(0)),
            ));
        }
        let before = self.sketch.snapshot();

        let line_corners: Vec<(EntityId, [EntityId; 2])> = self
            .sketch
            .entities()
            .filter_map(|(line, entity)| match entity {
                Entity::Line { start, end } => Some((line, [*start, *end])),
                _ => None,
            })
            .collect();
        let relation_operands: Vec<Vec<EntityId>> = self
            .sketch
            .constraints()
            .map(|(_, constraint)| constraint.referenced_entities())
            .collect();
        let mut removed = Vec::new();
        for id in existing {
            removed.extend(self.sketch.remove_entity(id));
        }
        let affected: BTreeSet<EntityId> = removed
            .iter()
            .copied()
            .chain(
                line_corners
                    .iter()
                    .filter(|(line, _)| removed.contains(line))
                    .flat_map(|(_, corners)| corners.iter().copied()),
            )
            .chain(
                relation_operands
                    .iter()
                    .filter(|operands| operands.iter().any(|id| removed.contains(id)))
                    .flatten()
                    .copied(),
            )
            .collect();
        removed.extend(self.sketch.sweep_detached_span_midpoints(&affected));
        removed.sort();
        removed.dedup();
        self.recompute();
        self.push_command(before);
        Ok(DeleteEntityResult {
            removed,
            sketch: self.dto(),
        })
    }

    /// Current unknown values of an entity in solver layout (Fix targets).
    fn unknown_values(&self, entity: EntityId) -> Vec<f64> {
        match self.sketch.entity(entity) {
            Some(Entity::Point { position }) => vec![position.x, position.y],
            Some(Entity::Circle { center, radius }) => vec![center.x, center.y, *radius],
            Some(Entity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            }) => vec![center.x, center.y, *radius, *start_angle, *end_angle],
            Some(Entity::Line { .. }) => {
                if let Some((a, b)) = self.sketch.resolved_line(entity) {
                    vec![a.x, a.y, b.x, b.y]
                } else {
                    vec![]
                }
            }
            Some(Entity::Spline { points }) => {
                points.iter().flat_map(|point| [point.x, point.y]).collect()
            }
            None => vec![],
        }
    }

    /// Kind-combination validation for panel application.
    fn validate_constraint(&self, constraint: &Constraint) -> Result<(), SessionError> {
        let entity = |id: EntityId| self.sketch.entity(id);
        let kinds_of = |ids: &[EntityId]| -> Vec<&'static str> {
            ids.iter()
                .map(|id| match entity(*id) {
                    Some(Entity::Point { .. }) => "point",
                    Some(Entity::Line { .. }) => "line",
                    Some(Entity::Circle { .. }) => "circle",
                    Some(Entity::Arc { .. }) => "arc",
                    Some(Entity::Spline { .. }) => "spline",
                    None => "missing",
                })
                .collect()
        };
        let invalid = |msg: &str| SessionError::InvalidConstraint(msg.to_string());

        match *constraint {
            Constraint::ArcEndpointCoincident { .. }
            | Constraint::ArcAngle { .. }
            | Constraint::OriginCoincident { .. }
            | Constraint::EqualDistance { .. }
            | Constraint::ReferenceMidpoint { .. }
            | Constraint::ReferenceOnEdge { .. }
            | Constraint::SpanMidpoint { .. } => {
                return Err(invalid(
                    "This relation is internal and is created by its sketch tool",
                ));
            }
            Constraint::CenterCoincident { point, curve } => {
                if !matches!(entity(point), Some(Entity::Point { .. }))
                    || !matches!(
                        entity(curve),
                        Some(Entity::Circle { .. } | Entity::Arc { .. })
                    )
                {
                    return Err(invalid(
                        "Center coincidence needs a point and a circle or arc",
                    ));
                }
            }
            Constraint::Horizontal { entity: e } | Constraint::Vertical { entity: e } => {
                if !matches!(entity(e), Some(Entity::Line { .. })) {
                    return Err(invalid("Horizontal/Vertical applies to a line"));
                }
            }
            Constraint::HorizontalPoints { a, b } | Constraint::VerticalPoints { a, b } => {
                if kinds_of(&[a, b]) != ["point", "point"] {
                    return Err(invalid("Point alignment needs two points"));
                }
            }
            Constraint::Fix { entity: e } => {
                if entity(e).is_none() {
                    return Err(invalid("Fix applies to an existing entity"));
                }
            }
            Constraint::Coincident { a, b } => {
                let ks = kinds_of(&[a, b]);
                let ok = matches!(
                    ks.as_slice(),
                    ["point", "point"]
                        | ["point", "line"]
                        | ["line", "point"]
                        | ["point", "circle"]
                        | ["circle", "point"]
                        | ["point", "arc"]
                        | ["arc", "point"]
                        | ["circle", "circle"]
                        | ["circle", "arc"]
                        | ["arc", "circle"]
                        | ["arc", "arc"]
                );
                if !ok {
                    return Err(invalid(
                        "Coincident needs two points, or a point on a line/circle/arc",
                    ));
                }
            }
            Constraint::Midpoint { a, b } => {
                if kinds_of(&[a, b]) != ["point", "line"] {
                    return Err(invalid("Midpoint needs a point and a line"));
                }
            }
            Constraint::Equal { a, b } => {
                let ks = kinds_of(&[a, b]);
                let ok = matches!(
                    ks.as_slice(),
                    ["line", "line"]
                        | ["circle", "circle"]
                        | ["arc", "arc"]
                        | ["circle", "arc"]
                        | ["arc", "circle"]
                );
                if !ok {
                    return Err(invalid("Equal needs two lines or two circles/arcs"));
                }
            }
            Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Collinear { a, b } => {
                if kinds_of(&[a, b]) != ["line", "line"] {
                    return Err(invalid("This constraint needs two lines"));
                }
            }
            Constraint::Tangent { a, b } => {
                let ks = kinds_of(&[a, b]);
                let curved = |k: &str| k == "circle" || k == "arc";
                let ok = (ks[0] == "line" && curved(ks[1]))
                    || (curved(ks[0]) && ks[1] == "line")
                    || (curved(ks[0]) && curved(ks[1]));
                if !ok {
                    return Err(invalid(
                        "Tangent needs a line and a circle/arc, or two circles/arcs",
                    ));
                }
            }
            Constraint::Concentric { a, b } => {
                let ks = kinds_of(&[a, b]);
                let curved = |k: &str| k == "circle" || k == "arc";
                if !(curved(ks[0]) && curved(ks[1])) {
                    return Err(invalid("Concentric needs two circles/arcs"));
                }
            }
            Constraint::Symmetry { a, b, axis } => {
                let ks = kinds_of(&[a, b, axis]);
                let ok = (ks[0] == "point" && ks[1] == "point" && ks[2] == "line")
                    || (ks[0] == "line" && ks[1] == "line" && ks[2] == "line");
                if !ok {
                    return Err(invalid(
                        "Symmetry needs two points and an axis line, or two lines and an axis",
                    ));
                }
            }
            Constraint::Distance { from, to, .. } => {
                let kf = kinds_of(&[from])[0];
                let kt = to.map(|t| kinds_of(&[t])[0]);
                let curved = |kind: &str| kind == "circle" || kind == "arc";
                let ok = (kf == "line" && kt.is_none())
                    || (kf == "point" && kt == Some("point"))
                    || (kf == "point" && kt == Some("line"))
                    || (kf == "line" && kt == Some("point"))
                    || (kf == "line" && kt == Some("line"))
                    || kt.is_some_and(|kind| curved(kf) && curved(kind));
                if !ok {
                    return Err(invalid(
                        "Distance needs a line, two points, point+line, two lines, or two circles/arcs",
                    ));
                }
            }
            Constraint::Radius { entity: e, .. } | Constraint::Diameter { entity: e, .. } => {
                if !matches!(entity(e), Some(Entity::Circle { .. } | Entity::Arc { .. })) {
                    return Err(invalid("Radius/Diameter applies to a circle or arc"));
                }
            }
            Constraint::Angle { a, b, .. } => {
                let two_lines = kinds_of(&[a, b]) == ["line", "line"];
                let axis = b.0 == 0 && kinds_of(&[a]) == ["line"];
                if !two_lines && !axis {
                    return Err(invalid("Angle needs two lines"));
                }
            }
        }
        Ok(())
    }

    /// Apply a constraint from the CONSTRAINTS panel. Over-constraining
    /// input is rejected with an explicit conflict report (D4.2).
    pub fn add_constraint(
        &mut self,
        constraint: Constraint,
    ) -> Result<AddConstraintResult, SessionError> {
        self.validate_constraint(&constraint)?;
        self.reject_duplicate_relation(&constraint)?;
        let before = self.sketch.snapshot();

        let cid = self.sketch.add_constraint(constraint);
        if let Constraint::Fix { entity } = constraint {
            let targets = self.unknown_values(entity);
            self.sketch.set_fix_targets(cid, targets);
        }

        let analysis = self.solve_constraint_operation_with_recovery(&[constraint]);
        let new_residual = solver::constraint_residual(&self.sketch, cid);

        if !analysis.converged || new_residual > INCONSISTENT_EPS {
            let error = self.classify_constraint_failure(cid, constraint);
            self.sketch.restore(before);
            self.recompute();
            return Err(error);
        }
        if solver::constraints_are_redundant(&self.sketch, &[cid]) {
            let error = self.classify_redundant_constraint(cid, constraint);
            self.sketch.restore(before);
            self.recompute();
            return Err(error);
        }

        self.analysis = Some(analysis);
        self.push_command(before);
        Ok(AddConstraintResult {
            constraint_id: cid,
            sketch: self.dto(),
        })
    }

    /// Apply a panel-generated constraint set atomically. This is used for
    /// multi-line H/V and similar bulk actions so a late conflict cannot
    /// leave earlier constraints behind, and Undo removes the whole action.
    pub fn add_constraints(
        &mut self,
        constraints: Vec<Constraint>,
    ) -> Result<ToolResult, SessionError> {
        if constraints.is_empty() {
            return Err(SessionError::InvalidConstraint(
                "no constraints to apply".to_string(),
            ));
        }
        let mut unique = Vec::with_capacity(constraints.len());
        for constraint in &constraints {
            self.validate_constraint(constraint)?;
            self.reject_duplicate_relation(constraint)?;
            if unique
                .iter()
                .any(|existing: &Constraint| self.sketch.relations_equivalent(existing, constraint))
            {
                return Err(Self::duplicate_relation_error(constraint));
            }
            unique.push(*constraint);
        }
        let before = self.sketch.snapshot();
        let mut added = Vec::with_capacity(constraints.len());
        for constraint in constraints {
            let cid = self.sketch.add_constraint(constraint);
            if let Constraint::Fix { entity } = constraint {
                let targets = self.unknown_values(entity);
                self.sketch.set_fix_targets(cid, targets);
            }
            added.push((cid, constraint));
        }

        let added_constraints = added
            .iter()
            .map(|(_, constraint)| *constraint)
            .collect::<Vec<_>>();
        let analysis = self.solve_constraint_operation_with_recovery(&added_constraints);
        let rejected = added
            .iter()
            .find(|(cid, _)| solver::constraint_residual(&self.sketch, *cid) > INCONSISTENT_EPS);
        if !analysis.converged || rejected.is_some() {
            let (cid, constraint) = rejected
                .copied()
                .or_else(|| added.last().copied())
                .expect("non-empty batch");
            let error = self.classify_constraint_failure(cid, constraint);
            self.sketch.restore(before);
            self.recompute();
            return Err(error);
        }

        let mut kept = added.len();
        for &(cid, constraint) in added.iter().rev() {
            if !solver::constraints_are_redundant(&self.sketch, &[cid]) {
                continue;
            }

            if kept == 1 || constraint.kind() == crate::constraint::ConstraintKind::Dimensional {
                let error = self.classify_redundant_constraint(cid, constraint);
                self.sketch.restore(before);
                self.recompute();
                return Err(error);
            }
            self.sketch.remove_constraint(cid);
            kept -= 1;
        }

        self.analysis = Some(solver::analyze(&self.sketch));
        self.push_command(before);
        let entities = added
            .iter()
            .flat_map(|(_, constraint)| constraint.referenced_entities())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(ToolResult {
            entities,
            sketch: self.dto(),
        })
    }

    /// Remove a geometric constraint (panel-applied relations). Dimension
    /// annotations must use [`Self::delete_dimension`].
    pub fn delete_constraint(
        &mut self,
        cid: ConstraintId,
    ) -> Result<AddConstraintResult, SessionError> {
        let constraint = self
            .sketch
            .constraints()
            .find(|(id, _)| *id == cid)
            .map(|(_, c)| *c)
            .ok_or(SessionError::InvalidConstraint(format!(
                "constraint {cid:?} not found"
            )))?;
        if constraint.kind() == crate::constraint::ConstraintKind::Dimensional {
            return Err(SessionError::InvalidConstraint(
                "use delete_dimension for sketch dimensions".to_string(),
            ));
        }
        let before = self.sketch.snapshot();

        let owned_handle = match constraint {
            Constraint::CenterCoincident { point, .. } | Constraint::SpanMidpoint { point, .. } => {
                Some(point)
            }
            _ => None,
        };
        self.sketch.remove_constraint(cid);
        if let Some(point) = owned_handle {
            self.sketch.remove_unused_generated_points([point]);
        }
        self.recompute();
        self.push_command(before);
        Ok(AddConstraintResult {
            constraint_id: cid,
            sketch: self.dto(),
        })
    }

    /// Fix/Unfix toggle: removes an existing Fix on the entity, else adds
    /// one.
    pub fn toggle_fix(&mut self, entity: EntityId) -> Result<AddConstraintResult, SessionError> {
        if self.sketch.entity(entity).is_none() {
            return Err(SessionError::EntityNotFound(entity));
        }
        if let Some(cid) = self.sketch.fix_constraint_on(entity) {
            let before = self.sketch.snapshot();
            self.sketch.remove_constraint(cid);
            self.recompute();
            self.push_command(before);
            return Ok(AddConstraintResult {
                constraint_id: cid,
                sketch: self.dto(),
            });
        }
        self.add_constraint(Constraint::Fix { entity })
    }

    /// Multi-selection Fix/Unfix as one transaction and one undo record.
    pub fn toggle_fix_entities(
        &mut self,
        entities: Vec<EntityId>,
    ) -> Result<ToolResult, SessionError> {
        let entities: BTreeSet<EntityId> = entities.into_iter().collect();
        if entities.is_empty() {
            return Err(SessionError::InvalidConstraint(
                "Fix/Unfix needs at least one entity".to_string(),
            ));
        }
        for entity in &entities {
            if self.sketch.entity(*entity).is_none() {
                return Err(SessionError::EntityNotFound(*entity));
            }
        }
        let before = self.sketch.snapshot();
        let mut added = Vec::new();
        let mut removed = false;
        for entity in &entities {
            if let Some(cid) = self.sketch.fix_constraint_on(*entity) {
                self.sketch.remove_constraint(cid);
                removed = true;
            } else {
                let cid = self
                    .sketch
                    .add_constraint(Constraint::Fix { entity: *entity });
                let targets = self.unknown_values(*entity);
                self.sketch.set_fix_targets(cid, targets);
                added.push((cid, Constraint::Fix { entity: *entity }));
            }
        }
        let analysis = solver::solve(&mut self.sketch, &[]);
        if !analysis.converged {
            self.sketch.restore(before);
            self.recompute();
            return Err(SessionError::InvalidConstraint(
                "Fix/Unfix conflicts with existing constraints".to_string(),
            ));
        }

        let added_ids = added.iter().map(|(cid, _)| *cid).collect::<Vec<_>>();
        if !removed
            && !added_ids.is_empty()
            && solver::constraints_are_redundant(&self.sketch, &added_ids)
        {
            let (cid, constraint) = added[0];
            let error = self.classify_redundant_constraint(cid, constraint);
            self.sketch.restore(before);
            self.recompute();
            return Err(error);
        }
        self.analysis = Some(analysis);
        self.push_command(before);
        Ok(ToolResult {
            entities: entities.into_iter().collect(),
            sketch: self.dto(),
        })
    }

    fn duplicate_relation_error(constraint: &Constraint) -> SessionError {
        let message = if constraint.kind() == crate::constraint::ConstraintKind::Dimensional {
            format!(
                "A driving {} dimension already controls this measurement; edit or remove the existing dimension first",
                constraint.kind_str()
            )
        } else {
            format!(
                "The {} constraint already exists on the selected geometry",
                constraint.kind_str()
            )
        };
        SessionError::InvalidConstraint(message)
    }

    pub(crate) fn reject_duplicate_relation(
        &self,
        constraint: &Constraint,
    ) -> Result<(), SessionError> {
        if self
            .sketch
            .equivalent_driving_constraint(constraint, None)
            .is_some()
        {
            return Err(Self::duplicate_relation_error(constraint));
        }
        Ok(())
    }

    /// Solve freshly added relations using operation-local invariants. A
    /// direction tool keeps authored sizes, a size tool keeps authored
    /// directions, and a position tool keeps carrier shape. If the preferred
    /// behavior is incompatible with the wider graph, fall back to the pure
    /// constraint solve rather than rejecting an otherwise valid relation.
    /// Symmetry axes are hard operation datums and never participate in that
    /// unrestricted fallback unless the command directly edits the axis. A
    /// stalled symmetry solve also retries from the nearest exact symmetric
    /// point-pair configuration.
    pub(crate) fn solve_constraint_operation_with_recovery(
        &mut self,
        constraints: &[Constraint],
    ) -> Analysis {
        self.solve_constraint_operation_with_preferences(constraints, false)
    }

    fn solve_dimension_edit_with_recovery(&mut self, constraints: &[Constraint]) -> Analysis {
        self.solve_constraint_operation_with_preferences(constraints, true)
    }

    fn solve_constraint_operation_with_preferences(
        &mut self,
        constraints: &[Constraint],
        chamfer_edit: bool,
    ) -> Analysis {
        let stays = self.operation_stays(constraints, chamfer_edit);
        self.seed_constraint_projections(constraints, chamfer_edit);
        let seeded_state = self.sketch.snapshot();
        let first = if stays.is_empty() {
            solver::solve(&mut self.sketch, &[])
        } else {
            solver::solve_with_stays(&mut self.sketch, &[], &stays)
        };
        if first.converged {
            return first;
        }
        let mut failed = first;
        if !stays.is_empty() {
            let mut relaxed = stays.clone();

            let addressed = constraints
                .iter()
                .flat_map(Constraint::referenced_entities)
                .collect::<BTreeSet<_>>();
            let newly_selected_axes = constraints
                .iter()
                .filter_map(|constraint| match *constraint {
                    Constraint::Symmetry { axis, .. } => Some(axis),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            let symmetry_axes = self
                .sketch
                .constraints()
                .filter(|(cid, _)| !self.sketch.is_reference_dimension(cid))
                .filter_map(|(_, constraint)| match *constraint {
                    Constraint::Symmetry { axis, .. } => Some(axis),
                    _ => None,
                })
                .filter(|axis| {
                    if newly_selected_axes.contains(axis) {
                        return true;
                    }
                    let endpoint_addressed =
                        self.sketch
                            .line_endpoint_ids(*axis)
                            .is_some_and(|(start, end)| {
                                addressed.contains(&start) || addressed.contains(&end)
                            });
                    !addressed.contains(axis) && !endpoint_addressed
                })
                .collect::<BTreeSet<_>>();
            relaxed
                .line_midpoints
                .retain(|(line, _)| symmetry_axes.contains(line));
            relaxed.point_pair_midpoints.clear();
            relaxed.point_positions.clear();
            relaxed.curve_centers.clear();
            if !relaxed.is_empty() {
                self.sketch.restore(seeded_state.clone());
                let second = solver::solve_with_stays(&mut self.sketch, &[], &relaxed);
                if second.converged {
                    return second;
                }
            }

            let mut size_only = relaxed;
            size_only
                .line_angles
                .retain(|(line, _)| symmetry_axes.contains(line));
            size_only.point_pair_angles.clear();
            if !size_only.is_empty() {
                self.sketch.restore(seeded_state.clone());
                let third = solver::solve_with_stays(&mut self.sketch, &[], &size_only);
                if third.converged {
                    return third;
                }
            }

            if !symmetry_axes.is_empty() {
                self.sketch.restore(seeded_state);
                return failed;
            }

            self.sketch.restore(seeded_state.clone());
            let fallback = solver::solve(&mut self.sketch, &[]);
            if fallback.converged {
                return fallback;
            }
            failed = fallback;
        }
        self.sketch.restore(seeded_state);
        let mut seeded = false;
        for constraint in constraints {
            if let Constraint::Symmetry { a, b, axis } = *constraint {
                seeded |= self.seed_symmetry_projection(a, b, axis);
            }
        }
        if seeded {
            solver::solve(&mut self.sketch, &[])
        } else {
            failed
        }
    }

    /// Recognize an independent trimmed Chamfer before applying edit-local
    /// placement preferences; ambiguous or wider graphs keep the generic path.
    fn chamfer_edit_topology(&self, from: EntityId, to: EntityId) -> Option<ChamferEditTopology> {
        let relations: Vec<_> = self
            .sketch
            .constraints()
            .filter_map(|(_, constraint)| match *constraint {
                Constraint::EqualDistance { origin, a, b }
                    if (origin == from && (a == to || b == to))
                        || (origin == to && (a == from || b == from)) =>
                {
                    Some((origin, [a, b]))
                }
                _ => None,
            })
            .collect();
        if relations.len() != 1 {
            return None;
        }
        let (origin, cutbacks) = relations[0];
        if cutbacks[0] == cutbacks[1] || cutbacks.contains(&origin) {
            return None;
        }
        let mut carriers = [origin; 2];
        let mut far_points = [origin; 2];
        for (index, point) in cutbacks.iter().enumerate() {
            let owners: Vec<_> = self
                .sketch
                .entities()
                .filter_map(|(id, _)| {
                    let (start, end) = self.sketch.line_endpoint_ids(id)?;
                    let far = if start == *point {
                        end
                    } else if end == *point {
                        start
                    } else {
                        return None;
                    };
                    let incident = self.sketch.constraints().any(|(_, constraint)| {
                        matches!(*constraint, Constraint::Coincident { a, b }
                        if (a == origin && b == id) || (b == origin && a == id))
                    });
                    incident.then_some((id, far))
                })
                .collect();
            if owners.len() != 1 {
                return None;
            }
            (carriers[index], far_points[index]) = owners[0];
        }
        if carriers[0] == carriers[1]
            || far_points[0] == far_points[1]
            || far_points
                .iter()
                .any(|point| *point == origin || cutbacks.contains(point))
        {
            return None;
        }
        let connectors: Vec<_> = self
            .sketch
            .entities()
            .filter_map(|(id, _)| {
                let (start, end) = self.sketch.line_endpoint_ids(id)?;
                ((start == cutbacks[0] && end == cutbacks[1])
                    || (start == cutbacks[1] && end == cutbacks[0]))
                    .then_some(id)
            })
            .collect();
        if connectors.len() != 1 {
            return None;
        }
        let local = [
            origin,
            cutbacks[0],
            cutbacks[1],
            far_points[0],
            far_points[1],
            carriers[0],
            carriers[1],
            connectors[0],
        ];
        // Keep this preference local: independently related or shared geometry
        // follows the existing generic edit and recovery path.
        for (id, entity) in self.sketch.entities() {
            if let Entity::Line { start, end } = entity {
                if !local.contains(&id) && (local.contains(start) || local.contains(end)) {
                    return None;
                }
            }
        }
        for (_, constraint) in self.sketch.constraints() {
            if !constraint
                .referenced_entities()
                .iter()
                .any(|id| local.contains(id))
            {
                continue;
            }
            let allowed = match *constraint {
                Constraint::EqualDistance { origin: o, a, b } => o == origin && [a, b] == cutbacks,
                Constraint::Distance {
                    from: f,
                    to: Some(t),
                    ..
                } => (f == from && t == to) || (f == to && t == from),
                Constraint::Coincident { a, b } => {
                    (a == origin && carriers.contains(&b)) || (b == origin && carriers.contains(&a))
                }
                Constraint::Horizontal { entity } | Constraint::Vertical { entity } => {
                    carriers.contains(&entity)
                }
                Constraint::Perpendicular { a, b } | Constraint::Parallel { a, b } => {
                    carriers.contains(&a) && carriers.contains(&b)
                }
                Constraint::Fix { entity } | Constraint::OriginCoincident { entity } => {
                    entity == origin || far_points.contains(&entity)
                }
                _ => false,
            };
            if !allowed {
                return None;
            }
        }
        let corner = self.sketch.point_position(origin)?;
        for (index, point) in cutbacks.iter().enumerate() {
            let cut = self.sketch.point_position(*point)?;
            let far = self.sketch.point_position(far_points[index])?;
            let ray = cut - corner;
            let span = ray.x.hypot(ray.y);
            let remaining = cut.distance(far);
            if !span.is_finite()
                || span < MIN_LINE_LENGTH_MM
                || !remaining.is_finite()
                || remaining < MIN_LINE_LENGTH_MM
            {
                return None;
            }
        }
        Some(ChamferEditTopology {
            origin,
            cutbacks,
            carriers,
            far_points,
        })
    }

    /// Put a newly requested relation on the nearest exact geometric branch
    /// before the nonlinear solve. The solver remains authoritative and can
    /// move the wider constrained component, but it no longer needs to find
    /// a finite solution by walking through a scale/translation null space.
    fn seed_constraint_projections(&mut self, constraints: &[Constraint], chamfer_edit: bool) {
        fn set_point(sketch: &mut Sketch, point: EntityId, position: Vec2) -> bool {
            let Some(Entity::Point { position: target }) = sketch.entity_mut(point) else {
                return false;
            };
            *target = position;
            true
        }

        fn set_line_pose(
            sketch: &mut Sketch,
            line: EntityId,
            midpoint: Vec2,
            length: f64,
            angle: f64,
        ) -> bool {
            let Some((start, end)) = sketch.line_endpoint_ids(line) else {
                return false;
            };
            if !length.is_finite() || length < MIN_LINE_LENGTH_MM || !angle.is_finite() {
                return false;
            }
            let half = Vec2::new(angle.cos(), angle.sin()) * (length * 0.5);
            set_point(sketch, start, midpoint - half) && set_point(sketch, end, midpoint + half)
        }

        fn line_pose(sketch: &Sketch, line: EntityId) -> Option<(Vec2, f64, f64)> {
            let (start, end) = sketch.resolved_line(line)?;
            let direction = end - start;
            let length = direction.length();
            (length >= MIN_LINE_LENGTH_MM).then_some((
                (start + end) * 0.5,
                length,
                direction.y.atan2(direction.x),
            ))
        }

        fn nearest_parallel(reference: f64, current: f64) -> f64 {
            let opposite = reference + std::f64::consts::PI;
            if wrap_angle(current - reference).abs() <= wrap_angle(current - opposite).abs() {
                reference
            } else {
                opposite
            }
        }

        fn nearest_perpendicular(reference: f64, current: f64) -> f64 {
            let ccw = reference + std::f64::consts::FRAC_PI_2;
            let cw = reference - std::f64::consts::FRAC_PI_2;
            if wrap_angle(current - ccw).abs() <= wrap_angle(current - cw).abs() {
                ccw
            } else {
                cw
            }
        }

        fn wrap_angle(angle: f64) -> f64 {
            let mut wrapped = angle.rem_euclid(std::f64::consts::TAU);
            if wrapped > std::f64::consts::PI {
                wrapped -= std::f64::consts::TAU;
            }
            wrapped
        }

        fn curve_spec(sketch: &Sketch, entity: EntityId) -> Option<(Vec2, f64)> {
            match sketch.entity(entity) {
                Some(Entity::Circle { center, radius } | Entity::Arc { center, radius, .. }) => {
                    Some((*center, *radius))
                }
                _ => None,
            }
        }

        fn set_curve_center(sketch: &mut Sketch, entity: EntityId, target: Vec2) -> bool {
            match sketch.entity_mut(entity) {
                Some(Entity::Circle { center, .. } | Entity::Arc { center, .. }) => {
                    *center = target;
                    true
                }
                _ => false,
            }
        }

        fn set_curve_radius(sketch: &mut Sketch, entity: EntityId, target: f64) -> bool {
            if !target.is_finite() || target < MIN_LINE_LENGTH_MM {
                return false;
            }
            match sketch.entity_mut(entity) {
                Some(Entity::Circle { radius, .. } | Entity::Arc { radius, .. }) => {
                    *radius = target;
                    true
                }
                _ => false,
            }
        }

        fn point_line_distance_seed(
            sketch: &Sketch,
            point: EntityId,
            carrier: EntityId,
            target: f64,
        ) -> Option<Vec2> {
            let current = sketch.point_position(point)?;
            let (carrier_start, carrier_end) = sketch.resolved_line(carrier)?;
            let carrier_direction = carrier_end - carrier_start;
            let carrier_length = carrier_direction.length();
            if carrier_length < MIN_LINE_LENGTH_MM {
                return None;
            }
            let tangent = carrier_direction * (1.0 / carrier_length);
            let normal = Vec2::new(-tangent.y, tangent.x);

            sketch
                .entities()
                .filter_map(|(_, entity)| {
                    let Entity::Line { start, end } = *entity else {
                        return None;
                    };
                    let opposite = if start == point {
                        end
                    } else if end == point {
                        start
                    } else {
                        return None;
                    };
                    let anchor = sketch.point_position(opposite)?;
                    let authored = current - anchor;
                    let authored_length = authored.length();
                    if authored_length < MIN_LINE_LENGTH_MM {
                        return None;
                    }
                    let motion = authored * (1.0 / authored_length);
                    let denominator = motion.dot(normal);
                    if denominator.abs() < 1.0e-9 {
                        return None;
                    }
                    let along = (target - (anchor - carrier_start).dot(normal)) / denominator;
                    let candidate = anchor + motion * along;
                    candidate
                        .x
                        .is_finite()
                        .then_some((candidate, candidate.distance(current)))
                        .filter(|(candidate, _)| candidate.y.is_finite())
                })
                .min_by(|(_, first), (_, second)| first.total_cmp(second))
                .map(|(candidate, _)| candidate)
        }

        for constraint in constraints {
            match *constraint {
                Constraint::Horizontal { entity } => {
                    if let Some((midpoint, length, current)) = line_pose(&self.sketch, entity) {
                        let target = nearest_parallel(0.0, current);
                        set_line_pose(&mut self.sketch, entity, midpoint, length, target);
                    }
                }
                Constraint::Vertical { entity } => {
                    if let Some((midpoint, length, current)) = line_pose(&self.sketch, entity) {
                        let target = nearest_parallel(std::f64::consts::FRAC_PI_2, current);
                        set_line_pose(&mut self.sketch, entity, midpoint, length, target);
                    }
                }
                Constraint::HorizontalPoints { a, b } | Constraint::VerticalPoints { a, b } => {
                    let (Some(first), Some(second)) =
                        (self.sketch.point_position(a), self.sketch.point_position(b))
                    else {
                        continue;
                    };
                    let direction = second - first;
                    let length = direction.length();
                    if length < MIN_LINE_LENGTH_MM {
                        continue;
                    }
                    let midpoint = (first + second) * 0.5;
                    let current = direction.y.atan2(direction.x);
                    let reference = if matches!(constraint, Constraint::HorizontalPoints { .. }) {
                        0.0
                    } else {
                        std::f64::consts::FRAC_PI_2
                    };
                    let target = nearest_parallel(reference, current);
                    let rotation = target - current;
                    let cosine = rotation.cos();
                    let sine = rotation.sin();
                    let mut rigid_points = BTreeSet::from([a, b]);
                    for (_, entity) in self.sketch.entities() {
                        let Entity::Line { start, end } = *entity else {
                            continue;
                        };
                        if start == a || start == b || end == a || end == b {
                            rigid_points.extend([start, end]);
                        }
                    }
                    let transformed = rigid_points
                        .into_iter()
                        .filter_map(|point| {
                            let relative = self.sketch.point_position(point)? - midpoint;
                            Some((
                                point,
                                midpoint
                                    + Vec2::new(
                                        relative.x * cosine - relative.y * sine,
                                        relative.x * sine + relative.y * cosine,
                                    ),
                            ))
                        })
                        .collect::<Vec<_>>();
                    for (point, position) in transformed {
                        set_point(&mut self.sketch, point, position);
                    }
                }
                Constraint::Parallel { a, b }
                | Constraint::Perpendicular { a, b }
                | Constraint::Angle { a, b, .. }
                    if b != crate::entity::AXIS_SENTINEL =>
                {
                    let (Some((_, _, reference)), Some((midpoint, length, current))) =
                        (line_pose(&self.sketch, a), line_pose(&self.sketch, b))
                    else {
                        continue;
                    };
                    let target = match *constraint {
                        Constraint::Parallel { .. } => nearest_parallel(reference, current),
                        Constraint::Perpendicular { .. } => {
                            nearest_perpendicular(reference, current)
                        }
                        Constraint::Angle { value, .. } => reference + value.to_radians(),
                        _ => unreachable!(),
                    };
                    set_line_pose(&mut self.sketch, b, midpoint, length, target);
                }
                Constraint::Angle { a, b, value } if b == crate::entity::AXIS_SENTINEL => {
                    if let Some((midpoint, length, _)) = line_pose(&self.sketch, a) {
                        set_line_pose(&mut self.sketch, a, midpoint, length, value.to_radians());
                    }
                }
                Constraint::Collinear { a, b } => {
                    let (
                        Some((reference_midpoint, _, reference_angle)),
                        Some((target_midpoint, length, current)),
                    ) = (line_pose(&self.sketch, a), line_pose(&self.sketch, b))
                    else {
                        continue;
                    };
                    let reference_direction =
                        Vec2::new(reference_angle.cos(), reference_angle.sin());
                    let projected_midpoint = reference_midpoint
                        + reference_direction
                            * (target_midpoint - reference_midpoint).dot(reference_direction);
                    set_line_pose(
                        &mut self.sketch,
                        b,
                        projected_midpoint,
                        length,
                        nearest_parallel(reference_angle, current),
                    );
                }
                Constraint::Equal { a, b } => {
                    match (line_pose(&self.sketch, a), line_pose(&self.sketch, b)) {
                        (Some((_, reference_length, _)), Some((midpoint, _, angle))) => {
                            set_line_pose(&mut self.sketch, b, midpoint, reference_length, angle);
                        }
                        _ => {
                            if let (Some((_, radius)), Some(_)) =
                                (curve_spec(&self.sketch, a), curve_spec(&self.sketch, b))
                            {
                                set_curve_radius(&mut self.sketch, b, radius);
                            }
                        }
                    }
                }
                Constraint::Coincident { a, b } => match (
                    self.sketch.entity(a).cloned(),
                    self.sketch.entity(b).cloned(),
                ) {
                    (Some(Entity::Point { .. }), Some(Entity::Point { position })) => {
                        set_point(&mut self.sketch, a, position);
                    }
                    (Some(Entity::Point { position }), Some(Entity::Line { .. })) => {
                        if let Some((start, end)) = self.sketch.resolved_line(b) {
                            let direction = end - start;
                            let squared_length = direction.dot(direction);
                            if squared_length >= MIN_LINE_LENGTH_MM.powi(2) {
                                let along = (position - start).dot(direction) / squared_length;
                                set_point(&mut self.sketch, a, start + direction * along);
                            }
                        }
                    }
                    (Some(Entity::Line { .. }), Some(Entity::Point { position })) => {
                        if let Some((start, end)) = self.sketch.resolved_line(a) {
                            let direction = end - start;
                            let squared_length = direction.dot(direction);
                            if squared_length >= MIN_LINE_LENGTH_MM.powi(2) {
                                let along = (position - start).dot(direction) / squared_length;
                                set_point(&mut self.sketch, b, start + direction * along);
                            }
                        }
                    }
                    (Some(Entity::Point { position }), Some(Entity::Circle { center, radius }))
                    | (
                        Some(Entity::Point { position }),
                        Some(Entity::Arc { center, radius, .. }),
                    ) => {
                        let radial = position - center;
                        let direction = if radial.length() < MIN_LINE_LENGTH_MM {
                            Vec2::new(1.0, 0.0)
                        } else {
                            radial * (1.0 / radial.length())
                        };
                        set_point(&mut self.sketch, a, center + direction * radius);
                    }
                    (Some(Entity::Circle { center, radius }), Some(Entity::Point { position }))
                    | (
                        Some(Entity::Arc { center, radius, .. }),
                        Some(Entity::Point { position }),
                    ) => {
                        let radial = position - center;
                        let direction = if radial.length() < MIN_LINE_LENGTH_MM {
                            Vec2::new(1.0, 0.0)
                        } else {
                            radial * (1.0 / radial.length())
                        };
                        set_point(&mut self.sketch, b, center + direction * radius);
                    }
                    (
                        Some(Entity::Circle { center, .. } | Entity::Arc { center, .. }),
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                    ) => {
                        set_curve_center(&mut self.sketch, b, center);
                    }
                    _ => {}
                },
                Constraint::OriginCoincident { entity } => match self.sketch.entity(entity) {
                    Some(Entity::Point { .. }) => {
                        set_point(&mut self.sketch, entity, Vec2::ZERO);
                    }
                    Some(Entity::Circle { .. } | Entity::Arc { .. }) => {
                        set_curve_center(&mut self.sketch, entity, Vec2::ZERO);
                    }
                    _ => {}
                },
                Constraint::CenterCoincident { point, curve } => {
                    if let Some(position) = self.sketch.point_position(point) {
                        set_curve_center(&mut self.sketch, curve, position);
                    }
                }
                Constraint::Midpoint { a, b } => {
                    if let Some((start, end)) = self.sketch.resolved_line(b) {
                        set_point(&mut self.sketch, a, (start + end) * 0.5);
                    }
                }
                Constraint::Concentric { a, b } => {
                    if let Some((center, _)) = curve_spec(&self.sketch, a) {
                        set_curve_center(&mut self.sketch, b, center);
                    }
                }
                Constraint::Tangent { a, b } => {
                    let (line, curve) = match (self.sketch.entity(a), self.sketch.entity(b)) {
                        (
                            Some(Entity::Line { .. }),
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        ) => (Some(a), Some(b)),
                        (
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                            Some(Entity::Line { .. }),
                        ) => (Some(b), Some(a)),
                        _ => (None, None),
                    };
                    if let (Some(line), Some(curve)) = (line, curve) {
                        if let (Some((start, end)), Some((center, radius))) = (
                            self.sketch.resolved_line(line),
                            curve_spec(&self.sketch, curve),
                        ) {
                            let direction = end - start;
                            let length = direction.length();
                            if length >= MIN_LINE_LENGTH_MM {
                                let normal = Vec2::new(-direction.y / length, direction.x / length);
                                let signed = (center - start).dot(normal);
                                let projection = center - normal * signed;
                                let side = if signed < 0.0 { -1.0 } else { 1.0 };
                                set_curve_center(
                                    &mut self.sketch,
                                    curve,
                                    projection + normal * (side * radius),
                                );
                            }
                        }
                    } else if let (Some((first, first_radius)), Some((second, second_radius))) =
                        (curve_spec(&self.sketch, a), curve_spec(&self.sketch, b))
                    {
                        let radial = second - first;
                        let distance = radial.length();
                        let direction = if distance < MIN_LINE_LENGTH_MM {
                            Vec2::new(1.0, 0.0)
                        } else {
                            radial * (1.0 / distance)
                        };
                        let target = if solver::circle_tangent_is_external(
                            distance,
                            first_radius,
                            second_radius,
                        ) {
                            first_radius + second_radius
                        } else {
                            (first_radius - second_radius).abs()
                        };
                        set_curve_center(&mut self.sketch, b, first + direction * target);
                    }
                }
                Constraint::Distance { from, to, value } => match (
                    self.sketch.entity(from).cloned(),
                    to.and_then(|entity| self.sketch.entity(entity).cloned()),
                    to,
                ) {
                    (Some(Entity::Line { .. }), None, None) => {
                        if let Some((midpoint, _, angle)) = line_pose(&self.sketch, from) {
                            set_line_pose(&mut self.sketch, from, midpoint, value.abs(), angle);
                        }
                    }
                    (
                        Some(Entity::Point { position: first }),
                        Some(Entity::Point { position: second }),
                        Some(to),
                    ) => {
                        if chamfer_edit && constraints.len() == 1 {
                            if let Some(ChamferEditTopology {
                                origin, cutbacks, ..
                            }) = self.chamfer_edit_topology(from, to)
                            {
                                let corner = self.sketch.point_position(origin).unwrap();
                                let positions = cutbacks.map(|point| {
                                    let ray = self.sketch.point_position(point).unwrap() - corner;
                                    let span = ray.x.hypot(ray.y);
                                    corner + Vec2::new(ray.x / span, ray.y / span) * value.abs()
                                });
                                set_point(&mut self.sketch, cutbacks[0], positions[0]);
                                set_point(&mut self.sketch, cutbacks[1], positions[1]);
                                continue;
                            }
                        }
                        let radial = second - first;
                        let direction = if radial.length() < MIN_LINE_LENGTH_MM {
                            Vec2::new(1.0, 0.0)
                        } else {
                            radial * (1.0 / radial.length())
                        };
                        let midpoint = (first + second) * 0.5;
                        let half = direction * (value.abs() * 0.5);
                        set_point(&mut self.sketch, from, midpoint - half);
                        set_point(&mut self.sketch, to, midpoint + half);
                    }
                    (Some(Entity::Point { position }), Some(Entity::Line { .. }), Some(line)) => {
                        if let Some(candidate) =
                            point_line_distance_seed(&self.sketch, from, line, value)
                        {
                            set_point(&mut self.sketch, from, candidate);
                        } else if let Some((start, end)) = self.sketch.resolved_line(line) {
                            let direction = end - start;
                            let length = direction.length();
                            if length >= MIN_LINE_LENGTH_MM {
                                let tangent = direction * (1.0 / length);
                                let normal = Vec2::new(-tangent.y, tangent.x);
                                let projection = start + tangent * (position - start).dot(tangent);
                                set_point(&mut self.sketch, from, projection + normal * value);
                            }
                        }
                    }
                    (Some(Entity::Line { .. }), Some(Entity::Point { position }), Some(point)) => {
                        if let Some(candidate) =
                            point_line_distance_seed(&self.sketch, point, from, value)
                        {
                            set_point(&mut self.sketch, point, candidate);
                        } else if let Some((start, end)) = self.sketch.resolved_line(from) {
                            let direction = end - start;
                            let length = direction.length();
                            if length >= MIN_LINE_LENGTH_MM {
                                let tangent = direction * (1.0 / length);
                                let normal = Vec2::new(-tangent.y, tangent.x);
                                let projection = start + tangent * (position - start).dot(tangent);
                                set_point(&mut self.sketch, point, projection + normal * value);
                            }
                        }
                    }
                    (Some(Entity::Line { .. }), Some(Entity::Line { .. }), Some(target_line)) => {
                        if let (Some((start, end)), Some((target_midpoint, length, angle))) = (
                            self.sketch.resolved_line(from),
                            line_pose(&self.sketch, target_line),
                        ) {
                            let direction = end - start;
                            let direction_length = direction.length();
                            if direction_length < MIN_LINE_LENGTH_MM {
                                continue;
                            }
                            let unit = direction * (1.0 / direction_length);
                            let normal = Vec2::new(-unit.y, unit.x);
                            let signed = (target_midpoint - start).dot(normal);
                            let side = if signed < 0.0 { -1.0 } else { 1.0 };
                            let along = start + unit * (target_midpoint - start).dot(unit);
                            set_line_pose(
                                &mut self.sketch,
                                target_line,
                                along + normal * (side * value.abs()),
                                length,
                                angle,
                            );
                        }
                    }
                    (
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(target),
                    ) => {
                        if let Some((_, radius)) = curve_spec(&self.sketch, from) {
                            set_curve_radius(&mut self.sketch, target, radius + value);
                        }
                    }
                    _ => {}
                },
                Constraint::Symmetry { a, b, axis } => {
                    self.seed_symmetry_projection(a, b, axis);
                }
                Constraint::Fix { .. }
                | Constraint::Radius { .. }
                | Constraint::Diameter { .. }
                | Constraint::ArcAngle { .. }
                | Constraint::ReferenceMidpoint { .. }
                | Constraint::ReferenceOnEdge { .. }
                | Constraint::SpanMidpoint { .. }
                | Constraint::ArcEndpointCoincident { .. }
                | Constraint::EqualDistance { .. } => {}
                Constraint::Parallel { .. }
                | Constraint::Perpendicular { .. }
                | Constraint::Angle { .. } => {}
            }
        }

        let alignments = self
            .sketch
            .constraints()
            .filter(|(cid, _)| !self.sketch.is_reference_dimension(cid))
            .filter_map(|(_, constraint)| match *constraint {
                Constraint::HorizontalPoints { a, b } => Some((a, b, true)),
                Constraint::VerticalPoints { a, b } => Some((a, b, false)),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (reference, follower, horizontal) in alignments {
            let (Some(reference_position), Some(follower_position)) = (
                self.sketch.point_position(reference),
                self.sketch.point_position(follower),
            ) else {
                continue;
            };
            let delta = if horizontal {
                Vec2::new(0.0, reference_position.y - follower_position.y)
            } else {
                Vec2::new(reference_position.x - follower_position.x, 0.0)
            };
            if delta.length() <= 1.0e-12 {
                continue;
            }
            let mut rigid_points = BTreeSet::from([follower]);
            for (_, entity) in self.sketch.entities() {
                let Entity::Line { start, end } = *entity else {
                    continue;
                };
                if start == follower && end != reference {
                    rigid_points.insert(end);
                } else if end == follower && start != reference {
                    rigid_points.insert(start);
                }
            }
            let translated = rigid_points
                .into_iter()
                .filter_map(|point| Some((point, self.sketch.point_position(point)? + delta)))
                .collect::<Vec<_>>();
            for (point, position) in translated {
                set_point(&mut self.sketch, point, position);
            }
        }
    }

    /// Capture the authored properties that a newly applied tool does not
    /// semantically own. These are preferences for this one solve only; they
    /// are not hidden constraints and never reduce the sketch's reported DOF.
    fn operation_stays(
        &self,
        constraints: &[Constraint],
        chamfer_edit: bool,
    ) -> solver::SolveStays {
        fn add_line_shape(
            lengths: &mut BTreeSet<EntityId>,
            angles: &mut BTreeSet<EntityId>,
            line: EntityId,
        ) {
            lengths.insert(line);
            angles.insert(line);
        }

        fn add_line_pose(
            lengths: &mut BTreeSet<EntityId>,
            angles: &mut BTreeSet<EntityId>,
            midpoints: &mut BTreeSet<EntityId>,
            line: EntityId,
        ) {
            add_line_shape(lengths, angles, line);
            midpoints.insert(line);
        }

        let mut line_lengths = BTreeSet::new();
        let mut line_angles = BTreeSet::new();
        let mut line_midpoints = BTreeSet::new();
        let mut curve_radii = BTreeSet::new();
        let mut curve_centers = BTreeSet::new();
        let mut point_pair_distances = BTreeSet::new();
        let mut point_pair_angles = BTreeSet::new();
        let mut point_pair_midpoints = BTreeSet::new();
        let mut point_positions = BTreeSet::new();
        let mut moving_line_endpoints = BTreeSet::new();
        let mut direction_operation = false;

        let preferred_direction_pivot = |reference: EntityId, follower: EntityId| {
            let (reference_start, reference_end) = self.sketch.line_endpoint_ids(reference)?;
            let (follower_start, follower_end) = self.sketch.line_endpoint_ids(follower)?;
            if follower_start == reference_start || follower_start == reference_end {
                return Some(follower_start);
            }
            if follower_end == reference_start || follower_end == reference_end {
                return Some(follower_end);
            }
            let incidence = |point: EntityId| {
                self.sketch
                    .entities()
                    .filter(|(_, entity)| {
                        matches!(**entity, Entity::Line { start, end } if start == point || end == point)
                    })
                    .count()
            };
            let start_incidence = incidence(follower_start);
            let end_incidence = incidence(follower_end);
            if start_incidence > end_incidence {
                Some(follower_start)
            } else if end_incidence > start_incidence {
                Some(follower_end)
            } else {
                None
            }
        };

        for constraint in constraints {
            match *constraint {
                Constraint::Horizontal { entity } | Constraint::Vertical { entity } => {
                    line_lengths.insert(entity);
                    line_midpoints.insert(entity);
                    direction_operation = true;
                }
                Constraint::HorizontalPoints { a, b } | Constraint::VerticalPoints { a, b } => {
                    point_pair_distances.insert((a, b));
                    point_pair_midpoints.insert((a, b));
                    for (entity, geometry) in self.sketch.entities() {
                        if matches!(
                            *geometry,
                            Entity::Line { start, end }
                                if start == a || start == b || end == a || end == b
                        ) {
                            line_lengths.insert(entity);
                        }
                    }
                }
                Constraint::Parallel { a, b }
                | Constraint::Perpendicular { a, b }
                | Constraint::Angle { a, b, .. } => {
                    if b == crate::entity::AXIS_SENTINEL {
                        line_lengths.insert(a);
                        line_midpoints.insert(a);
                    } else {
                        add_line_pose(&mut line_lengths, &mut line_angles, &mut line_midpoints, a);
                        line_lengths.insert(b);
                        if let Some(pivot) = preferred_direction_pivot(a, b) {
                            point_positions.insert(pivot);
                        } else {
                            line_midpoints.insert(b);
                        }
                    }
                    direction_operation = true;
                }

                Constraint::Collinear { a, b } => {
                    line_lengths.extend([a, b]);
                    line_angles.insert(a);
                    line_midpoints.insert(a);
                }

                Constraint::Coincident { a, b } => {
                    match (self.sketch.entity(a), self.sketch.entity(b)) {
                        (Some(Entity::Point { .. }), Some(Entity::Point { .. })) => {
                            point_positions.insert(b);
                            moving_line_endpoints.insert(a);
                        }
                        (Some(Entity::Point { .. }), Some(Entity::Line { .. })) => {
                            add_line_shape(&mut line_lengths, &mut line_angles, b);
                            moving_line_endpoints.insert(a);
                        }
                        (Some(Entity::Line { .. }), Some(Entity::Point { .. })) => {
                            add_line_shape(&mut line_lengths, &mut line_angles, a);
                            moving_line_endpoints.insert(b);
                        }
                        (
                            Some(Entity::Point { .. }),
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        ) => {
                            curve_radii.insert(b);
                            curve_centers.insert(b);
                            moving_line_endpoints.insert(a);
                        }
                        (
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                            Some(Entity::Point { .. }),
                        ) => {
                            curve_radii.insert(a);
                            curve_centers.insert(a);
                            moving_line_endpoints.insert(b);
                        }
                        (
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        ) => {
                            curve_radii.extend([a, b]);
                            curve_centers.insert(a);
                        }
                        _ => {}
                    }
                }
                Constraint::OriginCoincident { entity } => match self.sketch.entity(entity) {
                    Some(Entity::Point { .. }) => {
                        moving_line_endpoints.insert(entity);
                    }
                    Some(Entity::Circle { .. } | Entity::Arc { .. }) => {
                        curve_radii.insert(entity);
                    }
                    _ => {}
                },
                Constraint::CenterCoincident { point, curve } => {
                    point_positions.insert(point);
                    curve_radii.insert(curve);
                }

                Constraint::ArcAngle { entity, .. } => {
                    curve_radii.insert(entity);
                    curve_centers.insert(entity);
                }
                Constraint::Midpoint { a: point, b: line } => {
                    add_line_shape(&mut line_lengths, &mut line_angles, line);
                    moving_line_endpoints.insert(point);
                }
                Constraint::Concentric { a, b } => {
                    curve_radii.extend([a, b]);
                    curve_centers.insert(a);
                }

                Constraint::Tangent { a, b } => {
                    match (self.sketch.entity(a), self.sketch.entity(b)) {
                        (
                            Some(Entity::Line { .. }),
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        ) => {
                            line_lengths.insert(a);
                            line_midpoints.insert(a);
                            curve_radii.insert(b);
                        }
                        (
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                            Some(Entity::Line { .. }),
                        ) => {
                            line_lengths.insert(b);
                            line_midpoints.insert(b);
                            curve_radii.insert(a);
                        }
                        (
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        ) => {
                            curve_radii.extend([a, b]);
                            curve_centers.insert(a);
                        }
                        _ => {}
                    }

                    for (_, relation) in self.sketch.constraints() {
                        let Constraint::Tangent {
                            a: first,
                            b: second,
                        } = *relation
                        else {
                            continue;
                        };
                        let peer = if first == a || first == b {
                            second
                        } else if second == a || second == b {
                            first
                        } else {
                            continue;
                        };
                        if matches!(self.sketch.entity(peer), Some(Entity::Line { .. })) {
                            line_lengths.insert(peer);
                        }
                    }
                }

                Constraint::Equal { a, b } => {
                    match (self.sketch.entity(a), self.sketch.entity(b)) {
                        (Some(Entity::Line { .. }), Some(Entity::Line { .. })) => {
                            line_lengths.insert(a);
                            line_angles.extend([a, b]);
                            line_midpoints.extend([a, b]);
                        }
                        (
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                            Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        ) => {
                            curve_radii.insert(a);
                            curve_centers.extend([a, b]);
                        }
                        _ => {}
                    }
                }

                Constraint::Distance { from, to, .. } => match (
                    self.sketch.entity(from),
                    to.and_then(|entity| self.sketch.entity(entity)),
                    to,
                ) {
                    (Some(Entity::Line { .. }), None, None) => {
                        line_angles.insert(from);
                        line_midpoints.insert(from);
                    }
                    (Some(Entity::Point { .. }), Some(Entity::Point { .. }), Some(to)) => {
                        if chamfer_edit && constraints.len() == 1 {
                            if let Some(ChamferEditTopology {
                                origin,
                                carriers,
                                far_points,
                                ..
                            }) = self.chamfer_edit_topology(from, to)
                            {
                                point_positions.insert(origin);
                                point_positions.extend(far_points);
                                line_angles.extend(carriers);
                                continue;
                            }
                        }
                        point_pair_angles.insert((from, to));
                        point_pair_midpoints.insert((from, to));
                    }
                    (Some(Entity::Point { .. }), Some(Entity::Line { .. }), Some(to)) => {
                        add_line_shape(&mut line_lengths, &mut line_angles, to);
                        moving_line_endpoints.insert(from);
                    }
                    (Some(Entity::Line { .. }), Some(Entity::Point { .. }), Some(to)) => {
                        add_line_shape(&mut line_lengths, &mut line_angles, from);
                        moving_line_endpoints.insert(to);
                    }
                    (Some(Entity::Line { .. }), Some(Entity::Line { .. }), Some(to)) => {
                        add_line_shape(&mut line_lengths, &mut line_angles, from);
                        add_line_shape(&mut line_lengths, &mut line_angles, to);
                        line_midpoints.insert(from);
                    }
                    (
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(to),
                    ) => {
                        curve_radii.insert(from);
                        curve_centers.extend([from, to]);
                    }
                    _ => {}
                },

                Constraint::Symmetry { a, b, axis } => {
                    add_line_pose(
                        &mut line_lengths,
                        &mut line_angles,
                        &mut line_midpoints,
                        axis,
                    );
                    if matches!(self.sketch.entity(a), Some(Entity::Line { .. })) {
                        line_lengths.insert(a);
                    } else {
                        for (entity, geometry) in self.sketch.entities() {
                            if matches!(
                                *geometry,
                                Entity::Line { start, end }
                                    if start == a || start == b || end == a || end == b
                            ) {
                                add_line_shape(&mut line_lengths, &mut line_angles, entity);
                            }
                        }
                        moving_line_endpoints.extend([a, b]);
                    }
                }

                Constraint::Radius { entity, .. } | Constraint::Diameter { entity, .. } => {
                    curve_centers.insert(entity);
                }
                Constraint::Fix { .. }
                | Constraint::ReferenceMidpoint { .. }
                | Constraint::ReferenceOnEdge { .. }
                | Constraint::SpanMidpoint { .. }
                | Constraint::ArcEndpointCoincident { .. }
                | Constraint::EqualDistance { .. } => {}
            }
        }

        let addressed = constraints
            .iter()
            .flat_map(Constraint::referenced_entities)
            .collect::<BTreeSet<_>>();
        let existing_symmetry_axes = self
            .sketch
            .constraints()
            .filter(|(cid, _)| !self.sketch.is_reference_dimension(cid))
            .filter_map(|(_, constraint)| match *constraint {
                Constraint::Symmetry { axis, .. } => Some(axis),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        for axis in existing_symmetry_axes {
            let endpoint_addressed = self
                .sketch
                .line_endpoint_ids(axis)
                .is_some_and(|(start, end)| addressed.contains(&start) || addressed.contains(&end));
            if !addressed.contains(&axis) && !endpoint_addressed {
                add_line_pose(
                    &mut line_lengths,
                    &mut line_angles,
                    &mut line_midpoints,
                    axis,
                );
            }
        }

        if direction_operation {
            for (entity, geometry) in self.sketch.entities() {
                match *geometry {
                    Entity::Line { .. } => {
                        line_lengths.insert(entity);
                    }
                    Entity::Circle { .. } | Entity::Arc { .. } => {
                        curve_radii.insert(entity);
                    }
                    _ => {}
                }
            }
        }

        for moving in moving_line_endpoints {
            for (_, geometry) in self.sketch.entities() {
                let Entity::Line { start, end } = *geometry else {
                    continue;
                };
                if start == moving && end != moving {
                    point_positions.insert(end);
                } else if end == moving && start != moving {
                    point_positions.insert(start);
                }
            }
        }

        solver::SolveStays {
            rigid_entities: Vec::new(),
            line_lengths: line_lengths
                .into_iter()
                .filter_map(|line| {
                    let (start, end) = self.sketch.resolved_line(line)?;
                    Some((line, start.distance(end)))
                })
                .collect(),
            line_midpoints: line_midpoints
                .into_iter()
                .filter_map(|line| {
                    let (start, end) = self.sketch.resolved_line(line)?;
                    Some((line, (start + end) * 0.5))
                })
                .collect(),
            line_angles: line_angles
                .into_iter()
                .filter_map(|line| {
                    let (start, end) = self.sketch.resolved_line(line)?;
                    let direction = end - start;
                    (direction.length() >= MIN_LINE_LENGTH_MM)
                        .then_some((line, direction.y.atan2(direction.x)))
                })
                .collect(),
            point_pair_distances: point_pair_distances
                .into_iter()
                .filter_map(|(a, b)| {
                    Some((
                        a,
                        b,
                        self.sketch
                            .point_position(a)?
                            .distance(self.sketch.point_position(b)?),
                    ))
                })
                .collect(),
            point_pair_angles: point_pair_angles
                .into_iter()
                .filter_map(|(a, b)| {
                    let direction =
                        self.sketch.point_position(b)? - self.sketch.point_position(a)?;
                    (direction.length() >= MIN_LINE_LENGTH_MM).then_some((
                        a,
                        b,
                        direction.y.atan2(direction.x),
                    ))
                })
                .collect(),
            point_pair_midpoints: point_pair_midpoints
                .into_iter()
                .filter_map(|(a, b)| {
                    Some((
                        a,
                        b,
                        (self.sketch.point_position(a)? + self.sketch.point_position(b)?) * 0.5,
                    ))
                })
                .collect(),
            point_positions: point_positions
                .into_iter()
                .filter_map(|point| Some((point, self.sketch.point_position(point)?)))
                .collect(),
            curve_radii: curve_radii
                .into_iter()
                .filter_map(|entity| match self.sketch.entity(entity) {
                    Some(Entity::Circle { radius, .. } | Entity::Arc { radius, .. }) => {
                        Some((entity, *radius))
                    }
                    _ => None,
                })
                .collect(),
            curve_centers: curve_centers
                .into_iter()
                .filter_map(|curve| match self.sketch.entity(curve) {
                    Some(Entity::Circle { center, .. } | Entity::Arc { center, .. }) => {
                        Some((curve, *center))
                    }
                    _ => None,
                })
                .collect(),
        }
    }

    fn seed_symmetry_projection(&mut self, a: EntityId, b: EntityId, axis: EntityId) -> bool {
        let Some((axis_start, axis_end)) = self.sketch.resolved_line(axis) else {
            return false;
        };
        let direction = axis_end - axis_start;
        let length = direction.length();
        if length < MIN_LINE_LENGTH_MM {
            return false;
        }
        let tangent = direction * (1.0 / length);
        let normal = Vec2::new(-tangent.y, tangent.x);
        let pairs = match (self.sketch.entity(a), self.sketch.entity(b)) {
            (Some(Entity::Point { .. }), Some(Entity::Point { .. })) => vec![(a, b)],
            (Some(Entity::Line { .. }), Some(Entity::Line { .. })) => {
                let (Some((a_start, a_end)), Some((b_start, b_end))) = (
                    self.sketch.line_endpoint_ids(a),
                    self.sketch.line_endpoint_ids(b),
                ) else {
                    return false;
                };
                vec![(a_start, b_start), (a_end, b_end)]
            }
            _ => return false,
        };

        let mut projections = Vec::with_capacity(pairs.len());
        for (first, second) in pairs {
            let (Some(first_position), Some(second_position)) = (
                self.sketch.point_position(first),
                self.sketch.point_position(second),
            ) else {
                return false;
            };
            let first_relative = first_position - axis_start;
            let second_relative = second_position - axis_start;
            let along = (first_relative.dot(tangent) + second_relative.dot(tangent)) * 0.5;
            let across = (first_relative.dot(normal) - second_relative.dot(normal)) * 0.5;
            projections.push((
                first,
                axis_start + tangent * along + normal * across,
                second,
                axis_start + tangent * along - normal * across,
            ));
        }
        for (first, first_position, second, second_position) in projections {
            if let Some(Entity::Point { position }) = self.sketch.entity_mut(first) {
                *position = first_position;
            }
            if let Some(Entity::Point { position }) = self.sketch.entity_mut(second) {
                *position = second_position;
            }
        }
        true
    }

    /// Classify a consistent zero-rank relation separately from a genuine
    /// contradiction. The related constraints are dependencies, not
    /// blockers: removing one of them makes the proposed relation independent.
    fn classify_redundant_constraint(
        &self,
        cid: ConstraintId,
        constraint: Constraint,
    ) -> SessionError {
        SessionError::RedundantConstraint {
            rejected: self.describe_constraint(cid),
            implied_by: self.find_redundancy_dependencies(cid, constraint),
        }
    }

    /// Existing relations in the same structural/constraint component as a
    /// proposed one. Carrier lines and their endpoint points belong to the
    /// same component even though the solver stores their unknowns on points.
    fn related_constraints(
        &self,
        new_cid: ConstraintId,
        new_constraint: Constraint,
    ) -> Vec<(ConstraintId, Constraint)> {
        let mut component_entities = new_constraint
            .referenced_entities()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let constraints = self
            .sketch
            .constraints()
            .filter(|(cid, _)| *cid != new_cid && !self.sketch.is_reference_dimension(cid))
            .map(|(cid, constraint)| (cid, *constraint))
            .collect::<Vec<_>>();
        let mut candidates = BTreeSet::new();
        loop {
            let mut changed = false;
            for (entity_id, entity) in self.sketch.entities() {
                let referenced = entity.referenced_entities();
                if component_entities.contains(&entity_id)
                    || referenced
                        .iter()
                        .any(|reference| component_entities.contains(reference))
                {
                    changed |= component_entities.insert(entity_id);
                    for reference in referenced {
                        changed |= component_entities.insert(reference);
                    }
                }
            }
            for (cid, constraint) in &constraints {
                let referenced = constraint.referenced_entities();
                if referenced
                    .iter()
                    .any(|entity| component_entities.contains(entity))
                {
                    changed |= candidates.insert(*cid);
                    for entity in referenced {
                        changed |= component_entities.insert(entity);
                    }
                }
            }
            if !changed {
                break;
            }
        }
        constraints
            .into_iter()
            .filter(|(cid, _)| candidates.contains(cid))
            .collect()
    }

    /// Name the constraints that make `new_constraint` dependent. For each
    /// related relation, remove it and compare the rank with and without the
    /// proposed relation. A positive gain in that reduced graph proves the
    /// existing relation participates in the dependency circuit.
    fn find_redundancy_dependencies(
        &self,
        new_cid: ConstraintId,
        new_constraint: Constraint,
    ) -> Vec<ConstraintDesc> {
        let related = self.related_constraints(new_cid, new_constraint);
        let mut dependencies = related
            .iter()
            .filter_map(|(cid, _)| {
                let with_new = solver::rank_excluding_constraints(&self.sketch, &[*cid]);
                let without_both =
                    solver::rank_excluding_constraints(&self.sketch, &[*cid, new_cid]);
                (with_new > without_both).then(|| self.describe_constraint(*cid))
            })
            .collect::<Vec<_>>();

        if dependencies.is_empty() {
            dependencies = related
                .iter()
                .map(|(cid, _)| self.describe_constraint(*cid))
                .collect();
        }
        dependencies
    }

    /// Return an over-constraint only when removing a named relation proves
    /// that it is a culprit. Otherwise report numerical non-convergence
    /// truthfully without fabricating a connected constraint list.
    pub(crate) fn classify_constraint_failure(
        &mut self,
        cid: ConstraintId,
        constraint: Constraint,
    ) -> SessionError {
        let rejected = self.describe_constraint(cid);
        let conflicts_with = self.find_conflicts(cid, constraint);
        if conflicts_with.is_empty() {
            SessionError::ConstraintSolveFailed { rejected }
        } else {
            SessionError::OverConstrained {
                rejected,
                conflicts_with,
            }
        }
    }

    /// Identify existing constraints that conflict with the fresh one.
    ///
    /// Constraints propagate through a connected geometry network, so only
    /// checking constraints that directly mention the dimension endpoints
    /// can misleadingly blame the fixed origin. Walk the whole connected
    /// component, then test candidates by re-solving without each one.
    fn find_conflicts(
        &mut self,
        new_cid: ConstraintId,
        new_constraint: Constraint,
    ) -> Vec<ConstraintDesc> {
        let constraints = self.related_constraints(new_cid, new_constraint);
        let candidates = constraints
            .iter()
            .map(|(cid, _)| *cid)
            .collect::<BTreeSet<_>>();

        let snapshot = self.sketch.snapshot();
        self.sketch.remove_constraint(new_cid);

        let base = solver::solve(&mut self.sketch, &[]);
        self.sketch.restore(snapshot);
        let base_fully_defined = base.converged
            && new_constraint.referenced_entities().iter().all(|entity| {
                if base.fully_defined(*entity) {
                    return true;
                }

                let references = self
                    .sketch
                    .entity(*entity)
                    .map(Entity::referenced_entities)
                    .unwrap_or_default();
                !references.is_empty()
                    && references
                        .iter()
                        .all(|reference| base.fully_defined(*reference))
            });

        let mut conflicts: Vec<(Constraint, ConstraintDesc)> = Vec::new();
        for (cid, candidate) in &constraints {
            if candidates.contains(cid)
                && self
                    .sketch
                    .relations_directly_conflict(&new_constraint, candidate)
            {
                conflicts.push((*candidate, self.describe_constraint(*cid)));
            }
        }

        if base_fully_defined {
            for cid in &candidates {
                if conflicts.iter().any(|(constraint, _)| {
                    constraints
                        .iter()
                        .find(|(candidate_id, _)| candidate_id == cid)
                        .is_some_and(|(_, candidate)| constraint.same_relation(candidate))
                }) {
                    continue;
                }
                let snapshot = self.sketch.snapshot();
                self.sketch.remove_constraint(*cid);
                let analysis = solver::solve(&mut self.sketch, &[]);
                let residual = solver::constraint_residual(&self.sketch, new_cid);
                self.sketch.restore(snapshot);
                if analysis.converged && residual <= INCONSISTENT_EPS {
                    let candidate = constraints
                        .iter()
                        .find(|(candidate_id, _)| candidate_id == cid)
                        .map(|(_, constraint)| *constraint)
                        .expect("candidate constraint exists");
                    if !conflicts
                        .iter()
                        .any(|(existing, _)| existing.same_relation(&candidate))
                    {
                        conflicts.push((candidate, self.describe_constraint(*cid)));
                    }
                }
            }
        }
        let non_anchor_conflicts = conflicts
            .iter()
            .map(|(_, description)| description)
            .filter(|description| description.kind != "fix")
            .cloned()
            .collect::<Vec<_>>();
        if !non_anchor_conflicts.is_empty() {
            return non_anchor_conflicts;
        }
        if conflicts.is_empty() && base_fully_defined {
            for (cid, candidate) in &constraints {
                if candidates.contains(cid) && matches!(candidate, Constraint::Fix { .. }) {
                    conflicts.push((*candidate, self.describe_constraint(*cid)));
                }
            }
        }
        conflicts
            .into_iter()
            .map(|(_, description)| description)
            .collect()
    }

    /// Human-readable description of a constraint for the conflict report.
    fn describe_constraint(&self, cid: ConstraintId) -> ConstraintDesc {
        let Some((_, constraint)) = self.sketch.constraints().find(|(id, _)| *id == cid) else {
            return ConstraintDesc {
                id: cid,
                kind: "unknown".to_string(),
                entities: Vec::new(),
            };
        };
        let entities = constraint
            .referenced_entities()
            .iter()
            .filter_map(|id| {
                let kind = match self.sketch.entity(*id)? {
                    Entity::Point { .. } => "Point",
                    Entity::Line { .. } => "Line",
                    Entity::Circle { .. } => "Circle",
                    Entity::Arc { .. } => "Arc",
                    Entity::Spline { .. } => "Spline",
                };
                Some(EntityDesc {
                    id: *id,
                    label: format!("{}{}", kind, id.0),
                })
            })
            .collect();
        ConstraintDesc {
            id: cid,
            kind: constraint.kind_str().to_string(),
            entities,
        }
    }

    fn push_command(&mut self, before: SketchSnapshot) {
        let after = self.sketch.snapshot();
        self.undo.push(Command { before, after });
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> Result<UndoResult, SessionError> {
        let Some(command) = self.undo.pop() else {
            return Err(SessionError::NothingToUndo);
        };
        self.sketch.restore(command.before.clone());
        self.recompute();
        self.redo.push(command);
        Ok(UndoResult { sketch: self.dto() })
    }

    pub fn redo(&mut self) -> Result<UndoResult, SessionError> {
        let Some(command) = self.redo.pop() else {
            return Err(SessionError::NothingToRedo);
        };
        self.sketch.restore(command.after.clone());
        self.recompute();
        self.undo.push(command);
        Ok(UndoResult { sketch: self.dto() })
    }

    pub fn dto(&self) -> SketchDto {
        let analysis = self.analysis();
        let fd = |id: EntityId| analysis.fully_defined(id);
        let entities = self
            .sketch
            .entities()
            .filter_map(|(id, e)| match e {
                Entity::Point { position } => Some(EntityDto::Point {
                    id,
                    position: *position,
                    fully_defined: fd(id),
                }),
                Entity::Line { start, end } => {
                    let (a, b) = self.sketch.resolved_line(id)?;
                    Some(EntityDto::Line {
                        id,
                        start_id: *start,
                        end_id: *end,
                        start: a,
                        end: b,
                        fully_defined: fd(id),
                        consumed: crate::solver::line_is_consumed_trim_carrier(&self.sketch, id),
                    })
                }
                Entity::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } => Some(EntityDto::Arc {
                    id,
                    center: *center,
                    radius: *radius,
                    start_angle: *start_angle,
                    end_angle: *end_angle,
                    fully_defined: fd(id),
                }),
                Entity::Circle { center, radius } => Some(EntityDto::Circle {
                    id,
                    center: *center,
                    radius: *radius,
                    fully_defined: fd(id),
                }),
                Entity::Spline { points } => Some(EntityDto::Spline {
                    id,
                    tessellation: crate::geomops::spline::tessellate_spline(points, 16),
                    points: points.clone(),
                    fully_defined: fd(id),
                }),
            })
            .collect();
        let constraints = self
            .sketch
            .constraints()
            .map(|(id, constraint)| ConstraintDto {
                id,
                constraint: self.sketch.effective_constraint(id, *constraint),
            })
            .collect();
        SketchDto {
            name: self.name.clone(),
            plane: self.plane,
            basis: self.display_basis(),
            edit_occurrence_id: self.edit_placement.map(|(occurrence, _)| occurrence),
            entities,
            constraints,
            reference_midpoints: self
                .reference_midpoints
                .iter()
                .map(|(edge_id, position)| ReferenceMidpointDto {
                    edge_id: *edge_id,
                    position: *position,
                })
                .collect(),
            projected_edges: self.projected_edges.clone(),
            dimensions: self.dimension_dtos(),
            dimension_style: self.dimension_style,
            grid_snap: self.grid_snap,
            dof: DofDto {
                value: analysis.dof,
                fully_defined: analysis.dof == 0 && analysis.unknowns > 0,
            },
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
        }
    }
}
