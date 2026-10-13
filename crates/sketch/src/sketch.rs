use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

use limo_cad_core::EdgeId;

use crate::constraint::{Constraint, ConstraintId, ConstraintKind};
use crate::entity::{Entity, EntityId};
use crate::geometry::Vec2;
use crate::params::{ParamId, ParamTable};

/// Result of [`Sketch::degrees_of_freedom`].
///
/// `value` is computed from the constraint solver's Jacobian rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DofReport {
    /// Remaining degrees of freedom.
    pub value: i32,
    /// Whether the sketch has no remaining degrees of freedom.
    pub fully_defined: bool,
}

/// Whether a sketch dimension controls geometry or only reports its solved
/// measurement. Reference dimensions are persisted annotations, but they do
/// not contribute equations to the constraint solver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionMode {
    #[default]
    Driving,
    Reference,
}

/// Errors returned by [`Sketch::solve`].
///
/// Over-constraining input is rejected with an explicit conflict report
/// naming the offending constraints/entities. Reference annotations remain
/// explicitly marked measurements and never participate in the solver.
#[derive(Debug, Clone, PartialEq)]
pub enum SolveError {
    /// The new constraint set is over-constrained; `conflicts` names the
    /// constraints involved in the conflict.
    OverConstrained { conflicts: Vec<ConstraintId> },
    /// The Newton iteration failed to converge.
    NumericalFailure,
}

impl fmt::Display for SolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SolveError::OverConstrained { conflicts } => {
                write!(f, "sketch is over-constrained (conflicts: {:?})", conflicts)
            }
            SolveError::NumericalFailure => write!(f, "sketch solver failed to converge"),
        }
    }
}

impl std::error::Error for SolveError {}

/// Full cloneable state of a [`Sketch`], used by the session command stack
/// for undo/redo. Cheap at sketch scale and exact by construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SketchSnapshot {
    #[serde(default, serialize_with = "serialize_constraint_map")]
    offset_sides: HashMap<ConstraintId, f64>,
    #[serde(default)]
    reference_edges: Vec<crate::dto::ProjectedEdgeDto>,
    entities: Vec<(EntityId, Entity)>,
    /// Missing on legacy projects: preserve their points conservatively.
    #[serde(default)]
    generated_points: BTreeSet<EntityId>,
    constraints: Vec<(ConstraintId, Constraint)>,
    #[serde(serialize_with = "serialize_constraint_map")]
    fix_targets: HashMap<ConstraintId, Vec<f64>>,
    params: ParamTable,
    #[serde(serialize_with = "serialize_constraint_map")]
    dim_params: HashMap<ConstraintId, ParamId>,
    #[serde(serialize_with = "serialize_constraint_map")]
    dim_placements: HashMap<ConstraintId, Vec2>,
    /// Added after the original driving-only snapshot format. Missing entries
    /// deserialize as driving dimensions for backwards compatibility.
    #[serde(default, serialize_with = "serialize_constraint_map")]
    dim_modes: HashMap<ConstraintId, DimensionMode>,
    next_entity: u64,
    next_constraint: u64,
}

/// Hash seeds change when saved sketches are loaded into another document or
/// MCP reader. Keep model exports byte-stable without changing solver lookup
/// storage or the schema used by existing archives.
fn serialize_constraint_map<S: serde::Serializer, V: Serialize>(
    map: &HashMap<ConstraintId, V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    map.iter().collect::<BTreeMap<_, _>>().serialize(serializer)
}

impl SketchSnapshot {
    pub(crate) fn set_reference_edges(&mut self, edges: &[crate::dto::ProjectedEdgeDto]) {
        self.reference_edges = edges.to_vec();
    }
    /// Refresh saved support-edge midpoint targets without discarding an
    /// active session's undo/redo history. Every historical state must use
    /// the same current external reference or Undo could reintroduce drift.
    pub(crate) fn refresh_reference_midpoints(&mut self, targets: &HashMap<EdgeId, Vec2>) {
        refresh_reference_midpoint_constraints(&mut self.constraints, targets);
    }
}

/// 2D sketch on a plane: entities plus constraints between them.
///
/// Entities and constraints keep insertion order and are addressed by
/// stable, monotonically increasing ids. Line endpoints are shared `Point`
/// entities (structural coincident, see [`Entity`]).
#[derive(Debug, Clone, Default)]
pub struct Sketch {
    offset_sides: HashMap<ConstraintId, f64>,
    reference_edges: Vec<crate::dto::ProjectedEdgeDto>,
    entities: Vec<(EntityId, Entity)>,
    generated_points: BTreeSet<EntityId>,
    constraints: Vec<(ConstraintId, Constraint)>,
    /// Fix-constraint pin targets (entity unknown values captured when the
    /// Fix was added), keyed by constraint id. The `Constraint::Fix`
    /// variant itself carries no values, so the solver reads targets here.
    fix_targets: HashMap<ConstraintId, Vec<f64>>,
    /// Named parameters (d1, d2, …) backing driving dimensions (D9).
    params: ParamTable,
    /// Dimensional constraint → driving parameter binding.
    dim_params: HashMap<ConstraintId, ParamId>,
    /// Dimensional constraint → annotation text position (sketch mm).
    dim_placements: HashMap<ConstraintId, Vec2>,
    /// Explicit driving/reference state for dimension annotations. Legacy
    /// dimensions without an entry remain driving by default.
    dim_modes: HashMap<ConstraintId, DimensionMode>,
    next_entity: u64,

    entity_id_high_water: u64,
    next_constraint: u64,
}

impl Sketch {
    pub(crate) fn set_reference_edges(&mut self, edges: &[crate::dto::ProjectedEdgeDto]) {
        self.reference_edges = edges.to_vec();
    }

    pub(crate) fn reference_edge(&self, edge: EdgeId) -> Option<&crate::dto::ProjectedEdgeDto> {
        self.reference_edges.iter().find(|e| e.edge_id == edge)
    }
    pub fn new() -> Self {
        Self::default()
    }

    /// Update persistent support-edge midpoint constraints from their stable
    /// edge ids. Returns true when the live solver target changed.
    pub(crate) fn refresh_reference_midpoints(&mut self, targets: &HashMap<EdgeId, Vec2>) -> bool {
        refresh_reference_midpoint_constraints(&mut self.constraints, targets)
    }

    pub fn add_entity(&mut self, entity: Entity) -> EntityId {
        self.next_entity = self.next_entity.max(self.entity_id_high_water) + 1;
        self.entity_id_high_water = self.next_entity;
        let id = EntityId(self.next_entity);
        self.entities.push((id, entity));
        id
    }

    /// A handle created as part of geometry, not by the standalone Point tool.
    pub(crate) fn add_generated_point(&mut self, position: Vec2) -> EntityId {
        let id = self.add_entity(Entity::Point { position });
        self.mark_generated_point(id);
        id
    }

    pub(crate) fn mark_generated_point(&mut self, id: EntityId) {
        debug_assert!(matches!(self.entity(id), Some(Entity::Point { .. })));
        self.generated_points.insert(id);
    }

    /// An explicit Point-tool placement on an existing handle adopts it.
    pub(crate) fn retain_point(&mut self, id: EntityId) -> bool {
        self.generated_points.remove(&id)
    }

    /// Only generated, unused handles are disposable. In particular, a user
    /// point acquired by an arc is not owned by that arc. Check *all* relations,
    /// not just incident lines, before removing a now-unused tool handle.
    pub(crate) fn remove_unused_generated_points(
        &mut self,
        candidates: impl IntoIterator<Item = EntityId>,
    ) -> Vec<EntityId> {
        let mut removed = Vec::new();
        for point in candidates {
            if self.generated_points.contains(&point)
                && !self
                    .entities
                    .iter()
                    .any(|(_, e)| e.referenced_entities().contains(&point))
                && !self
                    .constraints
                    .iter()
                    .any(|(_, c)| c.referenced_entities().contains(&point))
            {
                removed.extend(self.remove_entity(point));
            }
        }
        removed
    }

    /// Whether a point is a tool-owned handle rather than user-authored.
    pub(crate) fn is_generated_point(&self, id: EntityId) -> bool {
        self.generated_points.contains(&id)
    }

    /// Whether any entity structurally references this point (a line endpoint).
    pub(crate) fn is_referenced_by_entity(&self, point: EntityId) -> bool {
        self.entities
            .iter()
            .any(|(_, entity)| entity.referenced_entities().contains(&point))
    }

    /// Every relation that mentions this point.
    pub(crate) fn relations_pointing_at(
        &self,
        point: EntityId,
    ) -> impl Iterator<Item = &Constraint> {
        self.constraints
            .iter()
            .map(|(_, constraint)| constraint)
            .filter(move |constraint| constraint.referenced_entities().contains(&point))
    }

    /// Retire `SpanMidpoint` relations whose diagonal anchors the last delete
    /// detached, so a half-erased center rectangle does not keep a corner no
    /// line uses.
    ///
    /// Only relations touching `affected` are considered: the caller passes the
    /// corners of the lines this delete removed, so an unrelated delete can
    /// never retire a live relation. A corner still anchors its relation when
    /// it is a live line endpoint, or when some relation other than a span
    /// midpoint still holds it — which is exactly how Fillet/Chamfer retain a
    /// trimmed corner. The center itself is left to the ordinary generated-point
    /// reap once its relation is gone.
    pub(crate) fn sweep_detached_span_midpoints(
        &mut self,
        affected: &BTreeSet<EntityId>,
    ) -> Vec<EntityId> {
        let anchored = |corner: EntityId| {
            self.is_referenced_by_entity(corner)
                || self
                    .relations_pointing_at(corner)
                    .any(|constraint| !matches!(constraint, Constraint::SpanMidpoint { .. }))
        };
        let stale: Vec<ConstraintId> = self
            .constraints
            .iter()
            .filter(|(_, constraint)| match *constraint {
                Constraint::SpanMidpoint { start, end, .. } => {
                    (affected.contains(&start) || affected.contains(&end))
                        && !(anchored(start) && anchored(end))
                }
                _ => false,
            })
            .map(|(id, _)| *id)
            .collect();
        if stale.is_empty() {
            return Vec::new();
        }
        let operands: Vec<EntityId> = self
            .constraints
            .iter()
            .filter(|(id, _)| stale.contains(id))
            .flat_map(|(_, constraint)| constraint.referenced_entities())
            .collect();
        self.constraints.retain(|(id, _)| !stale.contains(id));
        self.remove_unused_generated_points(operands)
    }

    /// Modify tools can detach/rebind handles without deleting the original
    /// curve. Clean up after their complete topology edit, not mid-edit, and
    /// never sweep unrelated points the user had already detached earlier.
    pub(crate) fn cleanup_generated_points(&mut self, before: &SketchSnapshot) {
        let candidates: BTreeSet<_> = before
            .entities
            .iter()
            .flat_map(|(_, e)| e.referenced_entities())
            .chain(
                before
                    .constraints
                    .iter()
                    .flat_map(|(_, c)| c.referenced_entities()),
            )
            .chain(
                self.generated_points
                    .difference(&before.generated_points)
                    .copied(),
            )
            .collect();
        self.remove_unused_generated_points(candidates);
    }

    /// Remove an entity, every entity that structurally references it
    /// (transitively — e.g. deleting a point deletes connected lines), and
    /// every constraint that references a removed entity.
    ///
    /// Returns the ids of all removed entities (including `id` itself), or
    /// an empty vec if `id` does not exist.
    pub fn remove_entity(&mut self, id: EntityId) -> Vec<EntityId> {
        if self.entity(id).is_none() {
            return Vec::new();
        }
        let mut removed = vec![id];

        loop {
            let before = removed.len();
            for (eid, entity) in &self.entities {
                if removed.contains(eid) {
                    continue;
                }
                if entity
                    .referenced_entities()
                    .iter()
                    .any(|r| removed.contains(r))
                {
                    removed.push(*eid);
                }
            }
            if removed.len() == before {
                break;
            }
        }
        let candidates: BTreeSet<_> = self
            .entities
            .iter()
            .filter(|(eid, _)| removed.contains(eid))
            .flat_map(|(_, e)| e.referenced_entities())
            .chain(
                self.constraints
                    .iter()
                    .filter(|(_, c)| c.referenced_entities().iter().any(|r| removed.contains(r)))
                    .flat_map(|(_, c)| c.referenced_entities()),
            )
            .collect();
        self.entities.retain(|(eid, _)| !removed.contains(eid));
        self.constraints
            .retain(|(_, c)| !c.referenced_entities().iter().any(|r| removed.contains(r)));
        self.generated_points.retain(|id| !removed.contains(id));
        removed.extend(self.remove_unused_generated_points(candidates));
        self.fix_targets
            .retain(|cid, _| self.constraints.iter().any(|(id, _)| id == cid));
        self.dim_params
            .retain(|cid, _| self.constraints.iter().any(|(id, _)| id == cid));
        self.dim_placements
            .retain(|cid, _| self.constraints.iter().any(|(id, _)| id == cid));
        self.dim_modes
            .retain(|cid, _| self.constraints.iter().any(|(id, _)| id == cid));
        self.offset_sides
            .retain(|cid, _| self.constraints.iter().any(|(id, _)| id == cid));
        removed
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities
            .iter()
            .find(|(eid, _)| *eid == id)
            .map(|(_, e)| e)
    }

    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.entities
            .iter_mut()
            .find(|(eid, _)| *eid == id)
            .map(|(_, e)| e)
    }

    pub fn entities(&self) -> impl Iterator<Item = (EntityId, &Entity)> {
        self.entities.iter().map(|(id, e)| (*id, e))
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Position of a `Point` entity.
    pub fn point_position(&self, id: EntityId) -> Option<Vec2> {
        match self.entity(id) {
            Some(Entity::Point { position }) => Some(*position),
            _ => None,
        }
    }

    /// Endpoint point ids of a `Line` entity.
    pub fn line_endpoint_ids(&self, id: EntityId) -> Option<(EntityId, EntityId)> {
        match self.entity(id) {
            Some(Entity::Line { start, end }) => Some((*start, *end)),
            _ => None,
        }
    }

    /// Resolved endpoint coordinates of a `Line` entity.
    pub fn resolved_line(&self, id: EntityId) -> Option<(Vec2, Vec2)> {
        let (start, end) = self.line_endpoint_ids(id)?;
        Some((self.point_position(start)?, self.point_position(end)?))
    }

    /// Ids of `Line` entities connected to the given point.
    pub fn lines_connected_to(&self, point_id: EntityId) -> Vec<EntityId> {
        self.entities
            .iter()
            .filter_map(|(eid, e)| match e {
                Entity::Line { start, end } if *start == point_id || *end == point_id => Some(*eid),
                _ => None,
            })
            .collect()
    }

    /// Find the point entity nearest to `p` within `tolerance`.
    pub fn nearest_point(&self, p: Vec2, tolerance: f64) -> Option<(EntityId, f64)> {
        let mut best: Option<(EntityId, f64)> = None;
        for (eid, e) in self.entities() {
            if let Entity::Point { position } = e {
                let d = position.distance(p);
                if d <= tolerance && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((eid, d));
                }
            }
        }
        best
    }

    /// Find the line whose midpoint is nearest to `p` within `tolerance`
    /// (M1d midpoint auto-snap). Returns the line id and midpoint coords.
    pub fn nearest_line_midpoint(&self, p: Vec2, tolerance: f64) -> Option<(EntityId, Vec2)> {
        let mut best: Option<(EntityId, Vec2, f64)> = None;
        for (eid, e) in self.entities() {
            if let Entity::Line { start, end } = e {
                let (Some(a), Some(b)) = (self.point_position(*start), self.point_position(*end))
                else {
                    continue;
                };
                let mid = (a + b) * 0.5;
                let d = mid.distance(p);
                if d <= tolerance && best.as_ref().is_none_or(|(_, _, bd)| d < *bd) {
                    best = Some((eid, mid, d));
                }
            }
        }
        best.map(|(id, mid, _)| (id, mid))
    }

    pub fn add_constraint(&mut self, constraint: Constraint) -> ConstraintId {
        self.next_constraint += 1;
        let id = ConstraintId(self.next_constraint);
        self.constraints.push((id, constraint));
        id
    }

    pub fn remove_constraint(&mut self, id: ConstraintId) -> Option<Constraint> {
        let index = self.constraints.iter().position(|(cid, _)| *cid == id)?;
        self.fix_targets.remove(&id);
        self.dim_params.remove(&id);
        self.dim_placements.remove(&id);
        self.dim_modes.remove(&id);
        self.offset_sides.remove(&id);
        Some(self.constraints.remove(index).1)
    }

    /// Replace one constraint without changing its stable id. Corner-edit
    /// topology migration uses this to retarget midpoint intent while keeping
    /// diagnostics and serialized references stable.
    pub fn replace_constraint(
        &mut self,
        id: ConstraintId,
        constraint: Constraint,
    ) -> Option<Constraint> {
        let (_, current) = self.constraints.iter_mut().find(|(cid, _)| *cid == id)?;
        Some(std::mem::replace(current, constraint))
    }

    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints
            .iter()
            .find(|(cid, _)| *cid == id)
            .map(|(_, c)| c)
    }

    pub fn constraints(&self) -> impl Iterator<Item = (ConstraintId, &Constraint)> {
        self.constraints.iter().map(|(id, c)| (*id, c))
    }

    /// Existing relation equivalent to `candidate`, with commutative entity
    /// order normalized. This protects the graph from exact duplicate
    /// geometric relations and multiple driving dimensions for one measure.
    pub fn equivalent_constraint(&self, candidate: &Constraint) -> Option<ConstraintId> {
        self.constraints.iter().find_map(|(id, constraint)| {
            self.relations_equivalent(constraint, candidate)
                .then_some(*id)
        })
    }

    /// Existing equivalent relation that participates in the solver. A
    /// reference dimension may duplicate the same measurement by design, so
    /// it must not block creation or conversion of the one allowed driver.
    pub fn equivalent_driving_constraint(
        &self,
        candidate: &Constraint,
        excluding: Option<ConstraintId>,
    ) -> Option<ConstraintId> {
        self.constraints.iter().find_map(|(id, constraint)| {
            if Some(*id) == excluding
                || (constraint.kind() == ConstraintKind::Dimensional
                    && self.is_reference_dimension(id))
            {
                return None;
            }
            self.relations_equivalent(constraint, candidate)
                .then_some(*id)
        })
    }

    pub fn equivalent_reference_constraint(
        &self,
        candidate: &Constraint,
        excluding: Option<ConstraintId>,
    ) -> Option<ConstraintId> {
        self.constraints.iter().find_map(|(id, constraint)| {
            (Some(*id) != excluding
                && constraint.kind() == ConstraintKind::Dimensional
                && self.is_reference_dimension(id)
                && self.relations_equivalent(constraint, candidate))
            .then_some(*id)
        })
    }

    /// Whether two proposed relations are equivalent in this sketch. Unlike
    /// `Constraint::same_relation`, this also resolves line carriers to their
    /// endpoint entities and is therefore suitable for atomic batch checks.
    pub fn relations_equivalent(&self, first: &Constraint, second: &Constraint) -> bool {
        first.same_relation(second) || self.same_carrier_relation(first, second)
    }

    /// Whether two relations are a proven algebraic contradiction, including
    /// the line-carrier versus endpoint-pair forms exposed by the UI.
    pub fn relations_directly_conflict(&self, first: &Constraint, second: &Constraint) -> bool {
        first.directly_conflicts_with(second)
            || self.carrier_relations_directly_conflict(first, second)
    }

    /// Recognize relations that use different public entity forms for the
    /// same underlying line endpoints. This closes the common loophole where
    /// a line-level H/V or length driver is duplicated by selecting its two
    /// endpoint points instead.
    fn same_carrier_relation(&self, first: &Constraint, second: &Constraint) -> bool {
        fn unordered_pair_eq(a: EntityId, b: EntityId, c: EntityId, d: EntityId) -> bool {
            (a == c && b == d) || (a == d && b == c)
        }

        let line_matches_pair = |line: EntityId, a: EntityId, b: EntityId| {
            self.line_endpoint_ids(line)
                .is_some_and(|(start, end)| unordered_pair_eq(start, end, a, b))
        };
        match (*first, *second) {
            (Constraint::Horizontal { entity: line }, Constraint::HorizontalPoints { a, b })
            | (Constraint::HorizontalPoints { a, b }, Constraint::Horizontal { entity: line })
            | (Constraint::Vertical { entity: line }, Constraint::VerticalPoints { a, b })
            | (Constraint::VerticalPoints { a, b }, Constraint::Vertical { entity: line }) => {
                line_matches_pair(line, a, b)
            }
            (
                Constraint::Distance {
                    from: line,
                    to: None,
                    ..
                },
                Constraint::Distance {
                    from: a,
                    to: Some(b),
                    ..
                },
            )
            | (
                Constraint::Distance {
                    from: a,
                    to: Some(b),
                    ..
                },
                Constraint::Distance {
                    from: line,
                    to: None,
                    ..
                },
            ) => line_matches_pair(line, a, b),
            _ => false,
        }
    }

    fn carrier_relations_directly_conflict(&self, first: &Constraint, second: &Constraint) -> bool {
        fn unordered_pair_eq(a: EntityId, b: EntityId, c: EntityId, d: EntityId) -> bool {
            (a == c && b == d) || (a == d && b == c)
        }

        let line_matches_pair = |line: EntityId, a: EntityId, b: EntityId| {
            self.line_endpoint_ids(line)
                .is_some_and(|(start, end)| unordered_pair_eq(start, end, a, b))
        };
        match (*first, *second) {
            (Constraint::Horizontal { entity: line }, Constraint::VerticalPoints { a, b })
            | (Constraint::VerticalPoints { a, b }, Constraint::Horizontal { entity: line })
            | (Constraint::Vertical { entity: line }, Constraint::HorizontalPoints { a, b })
            | (Constraint::HorizontalPoints { a, b }, Constraint::Vertical { entity: line }) => {
                line_matches_pair(line, a, b)
            }
            _ => false,
        }
    }

    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    /// Constraints of a given kind acting on an entity, e.g. the
    /// `Horizontal`/`Vertical` marks on a line (used by interim drag
    /// projection and by the glyph renderer).
    pub fn has_constraint_on(&self, entity: EntityId, pred: impl Fn(&Constraint) -> bool) -> bool {
        self.constraints
            .iter()
            .any(|(_, c)| c.referenced_entities().contains(&entity) && pred(c))
    }

    /// Register/lookup the pin targets of a Fix constraint (entity unknown
    /// values captured at add time).
    pub fn set_fix_targets(&mut self, id: ConstraintId, targets: Vec<f64>) {
        self.fix_targets.insert(id, targets);
    }

    pub fn fix_targets(&self, id: &ConstraintId) -> Option<&Vec<f64>> {
        self.fix_targets.get(id)
    }

    pub fn params(&self) -> &ParamTable {
        &self.params
    }

    pub fn params_mut(&mut self) -> &mut ParamTable {
        &mut self.params
    }

    /// Bind a dimensional constraint to its driving parameter and record
    /// the annotation's text position.
    pub fn bind_dimension(&mut self, cid: ConstraintId, param: ParamId, text_pos: Vec2) {
        self.dim_params.insert(cid, param);
        self.dim_placements.insert(cid, text_pos);
        self.dim_modes.insert(cid, DimensionMode::Driving);
    }

    /// Register a read-only dimension annotation. It deliberately has no
    /// parameter binding: its displayed value is measured from solved
    /// geometry instead of becoming a solver target.
    pub fn bind_reference_dimension(&mut self, cid: ConstraintId, text_pos: Vec2) {
        self.dim_params.remove(&cid);
        self.dim_placements.insert(cid, text_pos);
        self.dim_modes.insert(cid, DimensionMode::Reference);
    }

    pub fn dim_param(&self, cid: &ConstraintId) -> Option<ParamId> {
        self.dim_params.get(cid).copied()
    }

    pub fn dim_placement(&self, cid: &ConstraintId) -> Option<Vec2> {
        self.dim_placements.get(cid).copied()
    }

    pub fn dim_mode(&self, cid: &ConstraintId) -> DimensionMode {
        self.dim_modes.get(cid).copied().unwrap_or_default()
    }

    pub fn is_reference_dimension(&self, cid: &ConstraintId) -> bool {
        self.dim_mode(cid) == DimensionMode::Reference
    }

    pub fn set_dim_mode(&mut self, cid: ConstraintId, mode: DimensionMode) {
        self.dim_modes.insert(cid, mode);
    }

    pub fn unbind_dim_param(&mut self, cid: &ConstraintId) -> Option<ParamId> {
        self.dim_params.remove(cid)
    }

    pub fn set_dim_placement(&mut self, cid: ConstraintId, text_pos: Vec2) {
        self.dim_placements.insert(cid, text_pos);
    }

    /// The dimension constraint bound to a parameter, if any (used to
    /// clean up orphan params on dimension delete).
    pub fn dimension_of_param(&self, param: ParamId) -> Option<ConstraintId> {
        self.dim_params
            .iter()
            .find(|(_, p)| **p == param)
            .map(|(cid, _)| *cid)
    }

    /// Effective target value of a dimensional constraint: the bound
    /// parameter's evaluated value when present, else the literal stored
    /// on the constraint variant.
    pub fn dim_value(&self, cid: &ConstraintId, fallback: f64) -> f64 {
        self.dim_params
            .get(cid)
            .and_then(|pid| self.params.get(*pid))
            .map(|p| p.value)
            .unwrap_or(fallback)
    }

    pub(crate) fn offset_side(&self, cid: ConstraintId) -> Option<f64> {
        self.offset_sides.get(&cid).copied()
    }

    pub(crate) fn set_offset_side(&mut self, cid: ConstraintId, side: f64) {
        self.offset_sides.insert(cid, side);
    }

    pub(crate) fn measure_dimension(
        &self,
        cid: ConstraintId,
        constraint: Constraint,
    ) -> Option<f64> {
        if let (
            Some(side),
            Constraint::Distance {
                from, to: Some(to), ..
            },
        ) = (self.offset_side(cid), constraint)
        {
            let (a, b) = self.resolved_line(from)?;
            let (q, _) = self.resolved_line(to)?;
            let d = b - a;
            return Some((d.x * (q.y - a.y) - d.y * (q.x - a.x)) / d.length() / side);
        }
        self.measure_dimension_constraint(constraint)
    }

    /// Copy a constraint with its parameter-backed value materialized.
    pub fn effective_constraint(&self, cid: ConstraintId, constraint: Constraint) -> Constraint {
        let mut effective = constraint;
        let value = if self.is_reference_dimension(&cid) {
            self.measure_dimension(cid, constraint)
        } else {
            self.dim_params
                .get(&cid)
                .and_then(|pid| self.params.get(*pid))
                .map(|parameter| parameter.value)
        };
        if let Some(value) = value {
            effective.set_dimension_value(value);
        }
        effective
    }

    /// Measure a dimensional relation from current solved geometry. This is
    /// the authoritative value for reference dimensions and is also used to
    /// materialize truthful snapshots.
    pub fn measure_dimension_constraint(&self, constraint: Constraint) -> Option<f64> {
        fn signed_distance(point: Vec2, start: Vec2, end: Vec2) -> Option<f64> {
            let direction = end - start;
            let length = direction.length();
            (length >= 1e-12).then_some(
                (direction.x * (point.y - start.y) - direction.y * (point.x - start.x)) / length,
            )
        }

        fn line_angle(direction: Vec2) -> Option<f64> {
            (direction.length() >= 1e-12).then_some(direction.y.atan2(direction.x))
        }

        match constraint {
            Constraint::Distance { from, to: None, .. } => {
                let (start, end) = self.resolved_line(from)?;
                Some(start.distance(end))
            }
            Constraint::Distance {
                from, to: Some(to), ..
            } => match (self.entity(from), self.entity(to)) {
                (Some(Entity::Point { position: a }), Some(Entity::Point { position: b })) => {
                    Some(a.distance(*b))
                }
                (Some(Entity::Point { position }), Some(Entity::Line { .. })) => {
                    let (start, end) = self.resolved_line(to)?;
                    signed_distance(*position, start, end)
                }
                (Some(Entity::Line { .. }), Some(Entity::Point { position })) => {
                    let (start, end) = self.resolved_line(from)?;
                    signed_distance(*position, start, end)
                }
                (Some(Entity::Line { .. }), Some(Entity::Line { .. })) => {
                    let (start, end) = self.resolved_line(from)?;
                    let (other_start, _) = self.resolved_line(to)?;
                    signed_distance(other_start, start, end).map(f64::abs)
                }
                (
                    Some(
                        Entity::Circle {
                            radius: from_radius,
                            ..
                        }
                        | Entity::Arc {
                            radius: from_radius,
                            ..
                        },
                    ),
                    Some(
                        Entity::Circle {
                            radius: to_radius, ..
                        }
                        | Entity::Arc {
                            radius: to_radius, ..
                        },
                    ),
                ) => Some(to_radius - from_radius),
                _ => None,
            },
            Constraint::Radius { entity, .. } => match self.entity(entity) {
                Some(Entity::Circle { radius, .. } | Entity::Arc { radius, .. }) => Some(*radius),
                _ => None,
            },
            Constraint::Diameter { entity, .. } => match self.entity(entity) {
                Some(Entity::Circle { radius, .. } | Entity::Arc { radius, .. }) => {
                    Some(*radius * 2.0)
                }
                _ => None,
            },
            Constraint::ArcAngle { entity, .. } => match self.entity(entity) {
                Some(Entity::Arc {
                    start_angle,
                    end_angle,
                    ..
                }) => Some(crate::geometry::arc_span(*start_angle, *end_angle).to_degrees()),
                _ => None,
            },
            Constraint::Angle { a, b, .. } => {
                let (a_start, a_end) = self.resolved_line(a)?;
                let a_angle = line_angle(a_end - a_start)?;
                let b_angle = if b.0 == crate::entity::AXIS_SENTINEL.0 {
                    0.0
                } else {
                    let (b_start, b_end) = self.resolved_line(b)?;
                    line_angle(b_end - b_start)?
                };
                let mut delta = (a_angle - b_angle).to_degrees().abs() % 360.0;
                if delta > 180.0 {
                    delta = 360.0 - delta;
                }
                Some(delta)
            }
            _ => None,
        }
    }

    /// Synchronize the serialized fallback values after parameter evaluation.
    /// Expressions may update several dependent dimensions at once, so this
    /// deliberately refreshes every binding rather than only the edited one.
    pub fn sync_dimension_constraint_values(&mut self) {
        let updates = self
            .constraints
            .iter()
            .filter_map(|(cid, constraint)| {
                let value = if self.is_reference_dimension(cid) {
                    self.measure_dimension(*cid, *constraint)
                } else {
                    self.dim_params
                        .get(cid)
                        .and_then(|pid| self.params.get(*pid))
                        .map(|parameter| parameter.value)
                }?;
                Some((*cid, value))
            })
            .collect::<HashMap<_, _>>();
        for (cid, constraint) in &mut self.constraints {
            if let Some(value) = updates.get(cid) {
                constraint.set_dimension_value(*value);
            }
        }
    }

    /// The Fix constraint acting on an entity, if any (for Fix/Unfix).
    pub fn fix_constraint_on(&self, entity: EntityId) -> Option<ConstraintId> {
        self.constraints.iter().find_map(|(cid, c)| {
            if matches!(c, Constraint::Fix { entity: e } if *e == entity) {
                Some(*cid)
            } else {
                None
            }
        })
    }

    pub fn snapshot(&self) -> SketchSnapshot {
        SketchSnapshot {
            offset_sides: self.offset_sides.clone(),
            reference_edges: self.reference_edges.clone(),
            entities: self.entities.clone(),
            generated_points: self.generated_points.clone(),
            constraints: self
                .constraints
                .iter()
                .map(|(id, constraint)| (*id, self.effective_constraint(*id, *constraint)))
                .collect(),
            fix_targets: self.fix_targets.clone(),
            params: self.params.clone(),
            dim_params: self.dim_params.clone(),
            dim_placements: self.dim_placements.clone(),
            dim_modes: self.dim_modes.clone(),
            next_entity: self.next_entity,
            next_constraint: self.next_constraint,
        }
    }

    pub fn restore(&mut self, snapshot: SketchSnapshot) {
        self.offset_sides = snapshot.offset_sides;
        self.reference_edges = snapshot.reference_edges;
        self.entities = snapshot.entities;
        self.generated_points = snapshot.generated_points;
        self.constraints = snapshot.constraints;
        self.fix_targets = snapshot.fix_targets;
        self.params = snapshot.params;
        self.dim_params = snapshot.dim_params;
        self.dim_placements = snapshot.dim_placements;
        self.dim_modes = snapshot.dim_modes;

        for cid in self.dim_placements.keys() {
            self.dim_modes.entry(*cid).or_default();
        }
        self.next_entity = snapshot.next_entity;
        self.entity_id_high_water = self.entity_id_high_water.max(self.next_entity);
        self.next_constraint = snapshot.next_constraint;
        self.sync_dimension_constraint_values();
    }

    pub(crate) fn entity_id_high_water(&self) -> u64 {
        self.entity_id_high_water
    }

    pub(crate) fn reserve_entity_ids(&mut self, through: u64) {
        self.entity_id_high_water = self.entity_id_high_water.max(through);
    }

    /// Real DOF report from the constraint solver's rank analysis:
    /// `unknowns − rank(Jacobian)`.
    pub fn degrees_of_freedom(&self) -> DofReport {
        let analysis = crate::solver::analyze(self);
        DofReport {
            value: analysis.dof,
            fully_defined: analysis.dof == 0 && analysis.unknowns > 0,
        }
    }

    /// Solve the constraint system, updating entity geometry in place.
    /// Newton-based with damping (see [`crate::solver`]); returns
    /// [`SolveError::NumericalFailure`] when it does not converge.
    pub fn solve(&mut self) -> Result<(), SolveError> {
        let analysis = crate::solver::solve(self, &[]);
        if analysis.converged {
            self.sync_dimension_constraint_values();
            Ok(())
        } else {
            Err(SolveError::NumericalFailure)
        }
    }
}

fn refresh_reference_midpoint_constraints(
    constraints: &mut [(ConstraintId, Constraint)],
    targets: &HashMap<EdgeId, Vec2>,
) -> bool {
    let mut changed = false;
    for (_, constraint) in constraints {
        let Constraint::ReferenceMidpoint { edge, position, .. } = constraint else {
            continue;
        };
        let Some(target) = targets.get(edge).copied() else {
            continue;
        };
        if *position != target {
            *position = target;
            changed = true;
        }
    }
    changed
}

impl SketchSnapshot {
    /// Validate all stable-id references before a project snapshot is
    /// admitted into a live sketch session.
    pub(crate) fn validate(&self) -> Result<(), String> {
        use std::collections::HashSet;

        let mut entity_ids = HashSet::new();
        for (id, entity) in &self.entities {
            if id.0 == 0 || !entity_ids.insert(*id) {
                return Err(format!("duplicate or zero entity id {}", id.0));
            }
            if entity.referenced_entities().iter().any(|id| id.0 == 0) {
                return Err(format!("entity {} contains a zero reference", id.0));
            }
        }
        for (id, entity) in &self.entities {
            for reference in entity.referenced_entities() {
                if !entity_ids.contains(&reference) {
                    return Err(format!(
                        "entity {} references missing entity {}",
                        id.0, reference.0
                    ));
                }
                if !matches!(
                    self.entities
                        .iter()
                        .find(|(candidate, _)| *candidate == reference)
                        .map(|(_, entity)| entity),
                    Some(Entity::Point { .. })
                ) {
                    return Err(format!(
                        "entity {} references non-point endpoint {}",
                        id.0, reference.0
                    ));
                }
            }
        }
        if entity_ids.iter().map(|id| id.0).max().unwrap_or(0) > self.next_entity {
            return Err("next entity id is behind the saved entity table".to_string());
        }
        for point in &self.generated_points {
            if !self
                .entities
                .iter()
                .any(|(id, entity)| id == point && matches!(entity, Entity::Point { .. }))
            {
                return Err(format!("generated point {} is not a saved point", point.0));
            }
        }

        let mut constraint_ids = HashSet::new();
        let mut edge_ids = HashSet::new();
        for edge in &self.reference_edges {
            if !edge_ids.insert(edge.edge_id)
                || edge.points.len() < 2
                || edge
                    .points
                    .iter()
                    .any(|p| !p.x.is_finite() || !p.y.is_finite())
                || edge.circle.is_some_and(|c| {
                    !c.radius.is_finite()
                        || c.radius <= 0.0
                        || !c.center.x.is_finite()
                        || !c.center.y.is_finite()
                })
            {
                return Err("invalid cached support edge".into());
            }
        }
        if self
            .offset_sides
            .values()
            .any(|side| *side != 1.0 && *side != -1.0)
        {
            return Err("invalid signed offset direction".into());
        }
        for (id, constraint) in &self.constraints {
            if id.0 == 0 || !constraint_ids.insert(*id) {
                return Err(format!("duplicate or zero constraint id {}", id.0));
            }
            for reference in constraint.referenced_entities() {
                if reference.0 != 0 && !entity_ids.contains(&reference) {
                    return Err(format!(
                        "constraint {} references missing entity {}",
                        id.0, reference.0
                    ));
                }
            }
        }
        if constraint_ids.iter().map(|id| id.0).max().unwrap_or(0) > self.next_constraint {
            return Err("next constraint id is behind the saved constraint table".to_string());
        }
        if self
            .fix_targets
            .keys()
            .chain(self.dim_params.keys())
            .chain(self.dim_placements.keys())
            .chain(self.dim_modes.keys())
            .chain(self.offset_sides.keys())
            .any(|id| !constraint_ids.contains(id))
        {
            return Err("constraint metadata references a missing constraint".to_string());
        }
        for id in self.fix_targets.keys() {
            if !matches!(
                self.constraints
                    .iter()
                    .find(|(candidate, _)| candidate == id)
                    .map(|(_, constraint)| constraint),
                Some(Constraint::Fix { .. })
            ) {
                return Err(format!(
                    "fix metadata references non-Fix constraint {}",
                    id.0
                ));
            }
        }
        for id in self
            .dim_params
            .keys()
            .chain(self.dim_placements.keys())
            .chain(self.dim_modes.keys())
        {
            if !matches!(
                self.constraints
                    .iter()
                    .find(|(candidate, _)| candidate == id)
                    .map(|(_, constraint)| constraint.kind()),
                Some(crate::constraint::ConstraintKind::Dimensional)
            ) {
                return Err(format!(
                    "dimension metadata references non-dimensional constraint {}",
                    id.0
                ));
            }
        }
        self.params.validate()?;
        let parameter_ids = self
            .params
            .all()
            .iter()
            .map(|parameter| parameter.id)
            .collect::<HashSet<_>>();
        let mut referenced_parameters = HashSet::new();
        for id in self.dim_params.values() {
            if !parameter_ids.contains(id) {
                return Err("dimension references a missing parameter".to_string());
            }
            if !referenced_parameters.insert(*id) {
                return Err(format!(
                    "parameter {} is bound to more than one dimension",
                    id.0
                ));
            }
        }
        for cid in self.dim_placements.keys() {
            let mode = self.dim_modes.get(cid).copied().unwrap_or_default();
            match mode {
                DimensionMode::Driving => {
                    if !self.dim_params.contains_key(cid) {
                        return Err(format!(
                            "driving dimension {} has no parameter binding",
                            cid.0
                        ));
                    }
                }
                DimensionMode::Reference => {
                    if self.dim_params.contains_key(cid) {
                        return Err(format!(
                            "reference dimension {} has a driving parameter",
                            cid.0
                        ));
                    }
                }
            }
        }
        for cid in self.dim_modes.keys() {
            if !self.dim_placements.contains_key(cid) {
                return Err(format!(
                    "dimension {} has a mode but no annotation placement",
                    cid.0
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_dimension_and_fix_maps_keep_exact_json_after_restore() {
        let mut sketch = Sketch::new();
        for row in 0..8 {
            let y = row as f64 * 10.;
            let a = sketch.add_entity(Entity::point(0., y));
            let b = sketch.add_entity(Entity::point(10., y));
            let c = sketch.add_entity(Entity::point(0., y + 3.));
            let d = sketch.add_entity(Entity::point(10., y + 3.));
            let from = sketch.add_entity(Entity::line(a, b));
            let to = sketch.add_entity(Entity::line(c, d));
            let dimension = sketch.add_constraint(Constraint::Distance {
                from,
                to: Some(to),
                value: 3.,
            });
            let parameter = sketch
                .params_mut()
                .add(crate::params::ParamKind::Length, None, 3.)
                .unwrap();
            sketch.bind_dimension(dimension, parameter, Vec2::new(12., y + 1.5));
            sketch.set_offset_side(dimension, 1.);
            let fixed = sketch.add_constraint(Constraint::Fix { entity: a });
            sketch.set_fix_targets(fixed, vec![0., y]);
        }
        let snapshot = sketch.snapshot();
        snapshot.validate().unwrap();
        let expected = serde_json::to_string_pretty(&snapshot).unwrap();
        let expected_value: serde_json::Value = serde_json::from_str(&expected).unwrap();
        for _ in 0..16 {
            let decoded: SketchSnapshot = serde_json::from_str(&expected).unwrap();
            decoded.validate().unwrap();
            let mut reopened = Sketch::new();
            reopened.restore(decoded);
            let actual = serde_json::to_string_pretty(&reopened.snapshot()).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
                expected_value,
                "Restore must preserve every authored value"
            );
            assert_eq!(actual, expected, "Restore must preserve exact model JSON");
        }
    }

    /// Line (0,0)-(50,0) built from shared points, plus a circle.
    fn sample_sketch() -> (Sketch, EntityId, EntityId, EntityId, EntityId) {
        let mut s = Sketch::new();
        let p1 = s.add_entity(Entity::point(0.0, 0.0));
        let p2 = s.add_entity(Entity::point(50.0, 0.0));
        let line = s.add_entity(Entity::line(p1, p2));
        let circle = s.add_entity(Entity::circle(25.0, 20.0, 10.0));
        (s, p1, p2, line, circle)
    }

    #[test]
    fn entity_ids_are_unique_and_monotonic() {
        let (s, p1, p2, line, circle) = sample_sketch();
        assert!(p2.0 > p1.0 && line.0 > p2.0 && circle.0 > line.0);
        assert_eq!(s.entity_count(), 4);
    }

    #[test]
    fn equivalent_constraints_recognize_carrier_endpoint_forms() {
        let (mut s, p1, p2, line, circle) = sample_sketch();
        let horizontal = s.add_constraint(Constraint::Horizontal { entity: line });
        assert_eq!(
            s.equivalent_constraint(&Constraint::HorizontalPoints { a: p2, b: p1 }),
            Some(horizontal)
        );

        let length = s.add_constraint(Constraint::Distance {
            from: line,
            to: None,
            value: 50.0,
        });
        assert_eq!(
            s.equivalent_constraint(&Constraint::Distance {
                from: p1,
                to: Some(p2),
                value: 25.0,
            }),
            Some(length)
        );

        let radius = s.add_constraint(Constraint::Radius {
            entity: circle,
            value: 10.0,
        });
        assert_eq!(
            s.equivalent_constraint(&Constraint::Diameter {
                entity: circle,
                value: 20.0,
            }),
            Some(radius)
        );

        assert!(s.relations_directly_conflict(
            &Constraint::Horizontal { entity: line },
            &Constraint::VerticalPoints { a: p2, b: p1 },
        ));
    }

    #[test]
    fn entities_are_retrievable_and_mutable_by_id() {
        let (mut s, p1, _, line, _) = sample_sketch();
        assert!(matches!(s.entity(line), Some(Entity::Line { .. })));
        if let Some(Entity::Point { position }) = s.entity_mut(p1) {
            position.y = 5.0;
        }
        assert_eq!(s.point_position(p1), Some(Vec2::new(0.0, 5.0)));
        assert!(s.entity(EntityId(999)).is_none());
    }

    #[test]
    fn resolved_line_follows_shared_points() {
        let (mut s, p1, _, line, _) = sample_sketch();
        assert_eq!(
            s.resolved_line(line),
            Some((Vec2::new(0.0, 0.0), Vec2::new(50.0, 0.0)))
        );
        if let Some(Entity::Point { position }) = s.entity_mut(p1) {
            position.x = -10.0;
        }
        assert_eq!(
            s.resolved_line(line),
            Some((Vec2::new(-10.0, 0.0), Vec2::new(50.0, 0.0)))
        );
        assert_eq!(s.lines_connected_to(p1), vec![line]);
    }

    #[test]
    fn removing_a_point_cascades_to_connected_lines_and_constraints() {
        let (mut s, p1, _, line, circle) = sample_sketch();
        s.add_constraint(Constraint::Horizontal { entity: line });
        s.add_constraint(Constraint::Concentric { a: line, b: circle });
        s.add_constraint(Constraint::Radius {
            entity: circle,
            value: 10.0,
        });
        assert_eq!(s.constraint_count(), 3);

        let removed = s.remove_entity(p1);

        assert!(removed.contains(&p1) && removed.contains(&line));
        assert_eq!(s.entity_count(), 2);

        assert_eq!(s.constraint_count(), 1);
        assert!(matches!(
            s.constraints().next().map(|(_, c)| c),
            Some(Constraint::Radius { .. })
        ));
    }

    #[test]
    fn removing_a_line_keeps_its_endpoint_points() {
        let (mut s, p1, p2, line, _) = sample_sketch();
        let removed = s.remove_entity(line);
        assert_eq!(removed, vec![line]);
        assert!(s.entity(p1).is_some() && s.entity(p2).is_some());
    }

    #[test]
    fn remove_unknown_entity_is_a_no_op() {
        let (mut s, ..) = sample_sketch();
        assert!(s.remove_entity(EntityId(999)).is_empty());
    }

    #[test]
    fn constraint_bookkeeping_add_remove() {
        let (mut s, _, _, line, _) = sample_sketch();
        let c1 = s.add_constraint(Constraint::Horizontal { entity: line });
        let c2 = s.add_constraint(Constraint::Fix { entity: line });
        assert_ne!(c1, c2);
        assert_eq!(s.constraint_count(), 2);

        assert_eq!(
            s.remove_constraint(c1),
            Some(Constraint::Horizontal { entity: line })
        );
        assert_eq!(s.constraint_count(), 1);
        assert!(s.constraint(c1).is_none());
        assert!(s.constraint(c2).is_some());
        assert!(s.remove_constraint(c1).is_none());
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let (mut s, p1, _, line, _) = sample_sketch();
        s.add_constraint(Constraint::Horizontal { entity: line });
        let snap = s.snapshot();
        s.remove_entity(p1);
        assert_eq!(s.entity_count(), 2);
        s.restore(snap);
        assert_eq!(s.entity_count(), 4);
        assert!(s.entity(line).is_some());
        assert_eq!(s.constraint_count(), 1);

        let p3 = s.add_entity(Entity::point(1.0, 1.0));
        assert!(p3.0 > line.0);
    }

    #[test]
    fn reference_dimension_snapshot_roundtrip_preserves_mode_without_a_parameter() {
        let (mut sketch, _, _, line, _) = sample_sketch();
        let cid = sketch.add_constraint(Constraint::Distance {
            from: line,
            to: None,
            value: 0.0,
        });
        sketch.bind_reference_dimension(cid, Vec2::new(25.0, 10.0));
        let encoded = serde_json::to_string(&sketch.snapshot()).unwrap();
        let decoded: SketchSnapshot = serde_json::from_str(&encoded).unwrap();
        decoded.validate().unwrap();

        let mut restored = Sketch::new();
        restored.restore(decoded);
        assert_eq!(restored.dim_mode(&cid), DimensionMode::Reference);
        assert_eq!(restored.dim_param(&cid), None);
        assert!(matches!(
            restored.effective_constraint(cid, *restored.constraint(cid).unwrap()),
            Constraint::Distance { value, .. } if (value - 50.0).abs() < 1e-9
        ));
    }

    #[test]
    fn legacy_dimension_snapshot_without_modes_loads_as_driving() {
        let (mut sketch, _, _, line, _) = sample_sketch();
        let pid = sketch
            .params_mut()
            .add(crate::params::ParamKind::Length, None, 50.0)
            .unwrap();
        let cid = sketch.add_constraint(Constraint::Distance {
            from: line,
            to: None,
            value: 50.0,
        });
        sketch.bind_dimension(cid, pid, Vec2::new(25.0, 10.0));
        let mut encoded = serde_json::to_value(sketch.snapshot()).unwrap();
        encoded.as_object_mut().unwrap().remove("dim_modes");
        let decoded: SketchSnapshot = serde_json::from_value(encoded).unwrap();
        decoded.validate().unwrap();

        let mut restored = Sketch::new();
        restored.restore(decoded);
        assert_eq!(restored.dim_mode(&cid), DimensionMode::Driving);
        assert_eq!(restored.dim_param(&cid), Some(pid));
    }

    #[test]
    fn degrees_of_freedom_uses_solver_rank() {
        let (mut s, _, _, line, circle) = sample_sketch();
        s.remove_entity(circle);
        s.add_constraint(Constraint::Horizontal { entity: line });
        let dof = s.degrees_of_freedom();
        assert_eq!(dof.value, 3);
        assert!(!dof.fully_defined);
    }

    #[test]
    fn solve_converges_for_a_horizontal_line() {
        let (mut s, _, _, line, _) = sample_sketch();
        s.add_constraint(Constraint::Horizontal { entity: line });
        assert_eq!(s.solve(), Ok(()));
    }
}
