use serde::{Deserialize, Serialize};

use limo_cad_core::EdgeId;

use crate::entity::EntityId;
use crate::geometry::Vec2;

/// Stable identifier of a constraint within a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConstraintId(pub u64);

/// Classification of a constraint (geometric vs. dimensional), exposed for
/// UI badges and future solver partitioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    Geometric,
    Dimensional,
}

/// Which end of an arc (`ArcEndpointCoincident`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcEndpoint {
    Start,
    End,
}

/// Sketch constraint.
///
/// Covers the geometric constraints exposed by the sketcher, plus Fix/Unfix
/// and dimensional constraints. Variants carry the ids of the entities they
/// act on; dimensional variants also carry their value in
/// document units (mm and degrees for angles).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    Horizontal {
        entity: EntityId,
    },
    Vertical {
        entity: EntityId,
    },
    /// Keep two point entities on the same sketch-horizontal axis. This is
    /// the persistent form of temporary horizontal object-snap tracking.
    HorizontalPoints {
        a: EntityId,
        b: EntityId,
    },
    /// Keep two point entities on the same sketch-vertical axis. This is the
    /// persistent form of temporary vertical object-snap tracking.
    VerticalPoints {
        a: EntityId,
        b: EntityId,
    },
    Coincident {
        a: EntityId,
        b: EntityId,
    },
    /// Keep a point or circular-curve center at the sketch origin. This is
    /// the persistent result of an intentional origin snap; unlike `Fix`, it
    /// constrains only the two translational degrees of freedom.
    OriginCoincident {
        entity: EntityId,
    },
    /// Keep a circle/arc center on a selected sketch point. Point-on-curve
    /// coincidence has different semantics, so center acquisition needs its
    /// own visible, removable relation.
    CenterCoincident {
        point: EntityId,
        curve: EntityId,
    },
    Tangent {
        a: EntityId,
        b: EntityId,
    },
    Equal {
        a: EntityId,
        b: EntityId,
    },
    Parallel {
        a: EntityId,
        b: EntityId,
    },
    Perpendicular {
        a: EntityId,
        b: EntityId,
    },
    Fix {
        entity: EntityId,
    },
    Midpoint {
        a: EntityId,
        b: EntityId,
    },
    /// Keep a sketch point at the exact midpoint of a stable support-face
    /// edge. Unlike a one-time object snap, this external reference remains
    /// authoritative when dimensions are edited and is refreshed from the
    /// edge id whenever the face-hosted sketch is reopened.
    ReferenceMidpoint {
        point: EntityId,
        edge: EdgeId,
        position: Vec2,
    },
    /// A sliding relation to a finite support-face edge. The history-stage
    /// carrier is cached in the sketch snapshot and refreshed by stable id.
    ReferenceOnEdge {
        point: EntityId,
        edge: EdgeId,
    },
    /// Midpoint of an edge's original corner-to-corner span after a corner
    /// modifier trims one or both finite endpoints. `start` and `end` are
    /// the persistent corner reference points retained by Fillet/Chamfer.
    /// This keeps construction datums attached to the overall part envelope
    /// instead of silently shifting to the shortened carrier segment.
    SpanMidpoint {
        point: EntityId,
        start: EntityId,
        end: EntityId,
    },
    Concentric {
        a: EntityId,
        b: EntityId,
    },
    Collinear {
        a: EntityId,
        b: EntityId,
    },
    Symmetry {
        a: EntityId,
        b: EntityId,
        axis: EntityId,
    },
    /// Endpoint anchor (INTERNAL — created by arc/fillet/slot tools):
    /// a Point entity coincides with an arc's start/end point. Arc endpoints
    /// are implicit (center/radius/angles), so without this link a trimmed
    /// line endpoint is only glued by the infinite-line tangency — free to
    /// slide along the line when a driving dim pulls on it (2026-07-19 bug).
    ArcEndpointCoincident {
        point: EntityId,
        arc: EntityId,
        end: ArcEndpoint,
    },
    /// Equal distances from one origin point to two target points
    /// (INTERNAL — used by equal-distance Chamfer). Keeping this geometric
    /// relation separate from the single driving Distance dimension avoids
    /// duplicate dimension annotations while preserving both cutbacks.
    EqualDistance {
        origin: EntityId,
        a: EntityId,
        b: EntityId,
    },

    /// Distance between two entities, or from an entity to the sketch origin
    /// when `to` is `None`.
    Distance {
        from: EntityId,
        to: Option<EntityId>,
        value: f64,
    },
    Radius {
        entity: EntityId,
        value: f64,
    },
    Diameter {
        entity: EntityId,
        value: f64,
    },
    /// Angle in degrees between two entities.
    Angle {
        a: EntityId,
        b: EntityId,
        value: f64,
    },
    /// Included angle of an arc, in degrees. A center-point arc stores its own
    /// start and end angles, so its sweep is a real degree of freedom and can be
    /// dimensioned in its own right - which is what a typed sweep angle needs in
    /// order to stay editable afterwards.
    ArcAngle {
        entity: EntityId,
        value: f64,
    },
}

impl Constraint {
    /// Whether two constraints express the same geometric or dimensional
    /// relation. Operand order is normalized for commutative relations and
    /// dimensional target values are intentionally ignored: two driving
    /// dimensions cannot independently control the same measurement.
    pub fn same_relation(&self, other: &Self) -> bool {
        fn unordered_pair_eq(a: EntityId, b: EntityId, c: EntityId, d: EntityId) -> bool {
            (a == c && b == d) || (a == d && b == c)
        }

        match (*self, *other) {
            (Constraint::Horizontal { entity: a }, Constraint::Horizontal { entity: b })
            | (Constraint::Vertical { entity: a }, Constraint::Vertical { entity: b })
            | (
                Constraint::OriginCoincident { entity: a },
                Constraint::OriginCoincident { entity: b },
            )
            | (Constraint::Fix { entity: a }, Constraint::Fix { entity: b })
            | (Constraint::Radius { entity: a, .. }, Constraint::Radius { entity: b, .. })
            | (Constraint::Diameter { entity: a, .. }, Constraint::Diameter { entity: b, .. })
            | (Constraint::Radius { entity: a, .. }, Constraint::Diameter { entity: b, .. })
            | (Constraint::Diameter { entity: a, .. }, Constraint::Radius { entity: b, .. }) => {
                a == b
            }
            (
                Constraint::HorizontalPoints { a, b },
                Constraint::HorizontalPoints { a: c, b: d },
            )
            | (Constraint::VerticalPoints { a, b }, Constraint::VerticalPoints { a: c, b: d })
            | (Constraint::Coincident { a, b }, Constraint::Coincident { a: c, b: d })
            | (Constraint::Tangent { a, b }, Constraint::Tangent { a: c, b: d })
            | (Constraint::Equal { a, b }, Constraint::Equal { a: c, b: d })
            | (Constraint::Parallel { a, b }, Constraint::Parallel { a: c, b: d })
            | (Constraint::Perpendicular { a, b }, Constraint::Perpendicular { a: c, b: d })
            | (Constraint::Midpoint { a, b }, Constraint::Midpoint { a: c, b: d })
            | (Constraint::Concentric { a, b }, Constraint::Concentric { a: c, b: d })
            | (Constraint::Collinear { a, b }, Constraint::Collinear { a: c, b: d })
            | (Constraint::Angle { a, b, .. }, Constraint::Angle { a: c, b: d, .. }) => {
                unordered_pair_eq(a, b, c, d)
            }
            (Constraint::ArcAngle { entity: a, .. }, Constraint::ArcAngle { entity: b, .. }) => {
                a == b
            }
            (
                Constraint::Symmetry { a, b, axis },
                Constraint::Symmetry {
                    a: c,
                    b: d,
                    axis: other_axis,
                },
            ) => axis == other_axis && unordered_pair_eq(a, b, c, d),
            (
                Constraint::CenterCoincident { point, curve },
                Constraint::CenterCoincident {
                    point: other_point,
                    curve: other_curve,
                },
            ) => point == other_point && curve == other_curve,
            (
                Constraint::ArcEndpointCoincident { point, arc, end },
                Constraint::ArcEndpointCoincident {
                    point: other_point,
                    arc: other_arc,
                    end: other_end,
                },
            ) => point == other_point && arc == other_arc && end == other_end,
            (
                Constraint::ReferenceMidpoint { point, edge, .. },
                Constraint::ReferenceMidpoint {
                    point: other_point,
                    edge: other_edge,
                    ..
                },
            ) => point == other_point && edge == other_edge,
            (
                Constraint::ReferenceOnEdge { point, edge },
                Constraint::ReferenceOnEdge {
                    point: other_point,
                    edge: other_edge,
                },
            ) => point == other_point && edge == other_edge,
            (
                Constraint::SpanMidpoint { point, start, end },
                Constraint::SpanMidpoint {
                    point: other_point,
                    start: other_start,
                    end: other_end,
                },
            ) => point == other_point && unordered_pair_eq(start, end, other_start, other_end),
            (
                Constraint::EqualDistance { origin, a, b },
                Constraint::EqualDistance {
                    origin: other_origin,
                    a: c,
                    b: d,
                },
            ) => origin == other_origin && unordered_pair_eq(a, b, c, d),
            (
                Constraint::Distance { from, to, .. },
                Constraint::Distance {
                    from: other_from,
                    to: other_to,
                    ..
                },
            ) => match (to, other_to) {
                (None, None) => from == other_from,
                (Some(to), Some(other_to)) => unordered_pair_eq(from, to, other_from, other_to),
                _ => false,
            },
            _ => false,
        }
    }

    /// Relations that are algebraically incompatible on the same entities,
    /// independent of solver convergence. Keeping these small, explicit
    /// proofs separate from heuristic graph connectivity prevents false
    /// conflict accusations.
    pub fn directly_conflicts_with(&self, other: &Self) -> bool {
        fn unordered_pair_eq(a: EntityId, b: EntityId, c: EntityId, d: EntityId) -> bool {
            (a == c && b == d) || (a == d && b == c)
        }

        match (*self, *other) {
            (
                Constraint::Horizontal { entity: horizontal },
                Constraint::Vertical { entity: vertical },
            )
            | (
                Constraint::Vertical { entity: vertical },
                Constraint::Horizontal { entity: horizontal },
            ) => horizontal == vertical,
            (Constraint::HorizontalPoints { a, b }, Constraint::VerticalPoints { a: c, b: d })
            | (Constraint::VerticalPoints { a: c, b: d }, Constraint::HorizontalPoints { a, b })
            | (Constraint::Parallel { a, b }, Constraint::Perpendicular { a: c, b: d })
            | (Constraint::Perpendicular { a: c, b: d }, Constraint::Parallel { a, b }) => {
                unordered_pair_eq(a, b, c, d)
            }
            _ => false,
        }
    }

    /// Replace the serialized fallback value of a dimensional constraint.
    /// The bound parameter remains authoritative; keeping this copy in sync
    /// makes snapshots and flattened API DTOs truthful as well.
    pub fn set_dimension_value(&mut self, target: f64) {
        match self {
            Constraint::Distance { value, .. }
            | Constraint::Radius { value, .. }
            | Constraint::Diameter { value, .. }
            | Constraint::Angle { value, .. }
            | Constraint::ArcAngle { value, .. } => *value = target,
            _ => {}
        }
    }

    /// Stable snake_case kind string (matches the serde tag).
    pub fn kind_str(&self) -> &'static str {
        match self {
            Constraint::Horizontal { .. } => "horizontal",
            Constraint::Vertical { .. } => "vertical",
            Constraint::HorizontalPoints { .. } => "horizontal_points",
            Constraint::VerticalPoints { .. } => "vertical_points",
            Constraint::Coincident { .. } => "coincident",
            Constraint::OriginCoincident { .. } => "origin_coincident",
            Constraint::CenterCoincident { .. } => "center_coincident",
            Constraint::Tangent { .. } => "tangent",
            Constraint::Equal { .. } => "equal",
            Constraint::Parallel { .. } => "parallel",
            Constraint::Perpendicular { .. } => "perpendicular",
            Constraint::Fix { .. } => "fix",
            Constraint::Midpoint { .. } => "midpoint",
            Constraint::ReferenceMidpoint { .. } => "reference_midpoint",
            Constraint::ReferenceOnEdge { .. } => "reference_on_edge",
            Constraint::SpanMidpoint { .. } => "span_midpoint",
            Constraint::Concentric { .. } => "concentric",
            Constraint::Collinear { .. } => "collinear",
            Constraint::Symmetry { .. } => "symmetry",
            Constraint::ArcEndpointCoincident { .. } => "arc_endpoint_coincident",

            Constraint::ArcAngle { .. } => "angle",
            Constraint::EqualDistance { .. } => "equal_distance",
            Constraint::Distance { .. } => "distance",
            Constraint::Radius { .. } => "radius",
            Constraint::Diameter { .. } => "diameter",
            Constraint::Angle { .. } => "angle",
        }
    }

    pub fn kind(&self) -> ConstraintKind {
        match self {
            Constraint::Distance { .. }
            | Constraint::Radius { .. }
            | Constraint::Diameter { .. }
            | Constraint::Angle { .. }
            | Constraint::ArcAngle { .. } => ConstraintKind::Dimensional,
            _ => ConstraintKind::Geometric,
        }
    }

    /// All entities this constraint references (used to cascade deletes).
    pub fn referenced_entities(&self) -> Vec<EntityId> {
        match *self {
            Constraint::Horizontal { entity }
            | Constraint::Vertical { entity }
            | Constraint::OriginCoincident { entity }
            | Constraint::Fix { entity }
            | Constraint::Radius { entity, .. }
            | Constraint::Diameter { entity, .. } => vec![entity],
            Constraint::ReferenceMidpoint { point, .. } => vec![point],
            Constraint::ReferenceOnEdge { point, .. } => vec![point],
            Constraint::CenterCoincident { point, curve } => vec![point, curve],
            Constraint::Coincident { a, b }
            | Constraint::HorizontalPoints { a, b }
            | Constraint::VerticalPoints { a, b }
            | Constraint::Tangent { a, b }
            | Constraint::Equal { a, b }
            | Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Midpoint { a, b }
            | Constraint::Concentric { a, b }
            | Constraint::Collinear { a, b }
            | Constraint::Angle { a, b, .. } => vec![a, b],
            Constraint::ArcAngle { entity, .. } => vec![entity],
            Constraint::ArcEndpointCoincident { point, arc, .. } => vec![point, arc],
            Constraint::SpanMidpoint { point, start, end } => vec![point, start, end],
            Constraint::EqualDistance { origin, a, b } => vec![origin, a, b],
            Constraint::Symmetry { a, b, axis } => vec![a, b, axis],
            Constraint::Distance { from, to, .. } => match to {
                Some(to) => vec![from, to],
                None => vec![from],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_classification() {
        let e = EntityId(1);
        assert_eq!(
            Constraint::Horizontal { entity: e }.kind(),
            ConstraintKind::Geometric
        );
        assert_eq!(
            Constraint::Radius {
                entity: e,
                value: 5.0
            }
            .kind(),
            ConstraintKind::Dimensional
        );
    }

    #[test]
    fn referenced_entities_covers_all_variants() {
        let (a, b, c) = (EntityId(1), EntityId(2), EntityId(3));
        assert_eq!(
            Constraint::Symmetry { a, b, axis: c }.referenced_entities(),
            vec![a, b, c]
        );
        assert_eq!(
            Constraint::Distance {
                from: a,
                to: None,
                value: 10.0
            }
            .referenced_entities(),
            vec![a]
        );
        assert_eq!(
            Constraint::Distance {
                from: a,
                to: Some(b),
                value: 10.0
            }
            .referenced_entities(),
            vec![a, b]
        );
        assert_eq!(
            Constraint::OriginCoincident { entity: a }.referenced_entities(),
            vec![a]
        );
        assert_eq!(
            Constraint::CenterCoincident { point: a, curve: b }.referenced_entities(),
            vec![a, b]
        );
    }

    #[test]
    fn same_relation_normalizes_commutative_operands_and_dimension_values() {
        let (a, b, axis) = (EntityId(1), EntityId(2), EntityId(3));
        assert!(Constraint::Parallel { a, b }.same_relation(&Constraint::Parallel { a: b, b: a }));
        assert!(
            Constraint::Symmetry { a, b, axis }.same_relation(&Constraint::Symmetry {
                a: b,
                b: a,
                axis,
            })
        );
        assert!(Constraint::Distance {
            from: a,
            to: Some(b),
            value: 10.0,
        }
        .same_relation(&Constraint::Distance {
            from: b,
            to: Some(a),
            value: 25.0,
        }));
        assert!(!Constraint::Parallel { a, b }.same_relation(&Constraint::Perpendicular { a, b }));
        assert!(Constraint::Parallel { a, b }
            .directly_conflicts_with(&Constraint::Perpendicular { a: b, b: a }));
        assert!(Constraint::Radius {
            entity: a,
            value: 5.0,
        }
        .same_relation(&Constraint::Diameter {
            entity: a,
            value: 10.0,
        }));
        assert!(Constraint::OriginCoincident { entity: a }
            .same_relation(&Constraint::OriginCoincident { entity: a }));
        assert!(Constraint::CenterCoincident { point: a, curve: b }
            .same_relation(&Constraint::CenterCoincident { point: a, curve: b }));
        assert!(!Constraint::CenterCoincident { point: a, curve: b }
            .same_relation(&Constraint::CenterCoincident { point: b, curve: a }));
    }
}
