use serde::{Deserialize, Serialize};

use crate::geometry::Vec2;

/// Stable identifier of a sketch entity within a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub u64);

/// Sentinel entity id for "the plane's +u axis" in angle constraints
/// (auto-created axis angle dimensions, D9). Real entity ids start at 1,
/// so 0 can never collide.
pub const AXIS_SENTINEL: EntityId = EntityId(0);

/// 2D sketch entity.
///
/// Lines reference their endpoints as shared [`EntityId`]s of `Point`
/// entities: coincident endpoints are one structural point, so moving the
/// point moves every connected line. Angles are radians, CCW from +X.
/// Internally tagged so
/// JSON snapshots are self-describing (`{"type": "line", ...}`).
/// NOT Copy: the spline variant owns its fit-point list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entity {
    Point {
        position: Vec2,
    },
    Line {
        /// Entity id of the start `Point`.
        start: EntityId,
        /// Entity id of the end `Point`.
        end: EntityId,
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
    /// Fit-point spline (centripetal Catmull-Rom through the points, M1
    /// follow-up). Self-contained like Arc/Circle: fit points participate
    /// in Fix/DOF solving but are not shared point entities.
    Spline {
        points: Vec<Vec2>,
    },
}

impl Entity {
    pub fn point(x: f64, y: f64) -> Self {
        Self::Point {
            position: Vec2::new(x, y),
        }
    }

    /// Create a line between two existing point entities.
    pub fn line(start: EntityId, end: EntityId) -> Self {
        Self::Line { start, end }
    }

    pub fn circle(cx: f64, cy: f64, radius: f64) -> Self {
        Self::Circle {
            center: Vec2::new(cx, cy),
            radius,
        }
    }

    pub fn arc(cx: f64, cy: f64, radius: f64, start_angle: f64, end_angle: f64) -> Self {
        Self::Arc {
            center: Vec2::new(cx, cy),
            radius,
            start_angle,
            end_angle,
        }
    }

    /// Ids of entities this entity structurally references (line endpoints).
    /// Used to cascade deletes so no dangling references survive.
    pub fn referenced_entities(&self) -> Vec<EntityId> {
        match *self {
            Entity::Line { start, end } => vec![start, end],
            _ => Vec::new(),
        }
    }

    /// Naive parameter count used only by the DOF placeholder
    /// (`Sketch::degrees_of_freedom`). NOT the real solver's parameterization.
    /// Lines contribute nothing here: their geometry lives in the shared
    /// endpoint points (2 DOF each).
    #[allow(dead_code)]
    pub(crate) fn dof_contribution(&self) -> i32 {
        match self {
            Entity::Point { .. } => 2,
            Entity::Line { .. } => 0,
            Entity::Arc { .. } => 5,
            Entity::Circle { .. } => 3,

            Entity::Spline { points } => points.len() as i32 * 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_serializes_with_type_tag() {
        let e = Entity::line(EntityId(1), EntityId(2));
        let v = serde_json::to_value(e).unwrap();
        assert_eq!(v["type"], "line");
        assert_eq!(v["start"], 1);
        assert_eq!(v["end"], 2);
    }
}
