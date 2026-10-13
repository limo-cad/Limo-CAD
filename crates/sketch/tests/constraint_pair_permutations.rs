//! Pairwise constraint-order regression matrix.
//!
//! Every panel relation and every dimensional equation is applied both
//! before and after every other one on a shared mixed-geometry sketch.  A
//! successful pair must keep both relations exact and bounded; a rejected
//! second relation must leave the first operation completely untouched.

use std::collections::BTreeSet;
use std::f64::consts::PI;

use limo_cad_sketch::{
    CircleMode, Constraint, DimensionMode, DimensionRequest, EntityDto, EntityId, OriginPlane,
    PlaneRef, SketchDto, SketchSession, Vec2,
};

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};
const EPS: f64 = 2.0e-5;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Horizontal,
    Vertical,
    HorizontalPoints,
    VerticalPoints,
    Coincident,
    Tangent,
    EqualLines,
    EqualCurves,
    Parallel,
    Perpendicular,
    Fix,
    Midpoint,
    Concentric,
    Collinear,
    SymmetryPoints,
    SymmetryLines,
    LineLength,
    PointDistance,
    PointLineDistance,
    LineOffset,
    CurveOffset,
    Radius,
    Diameter,
    Angle,
    AxisAngle,
}

const OPERATIONS: [Operation; 25] = [
    Operation::Horizontal,
    Operation::Vertical,
    Operation::HorizontalPoints,
    Operation::VerticalPoints,
    Operation::Coincident,
    Operation::Tangent,
    Operation::EqualLines,
    Operation::EqualCurves,
    Operation::Parallel,
    Operation::Perpendicular,
    Operation::Fix,
    Operation::Midpoint,
    Operation::Concentric,
    Operation::Collinear,
    Operation::SymmetryPoints,
    Operation::SymmetryLines,
    Operation::LineLength,
    Operation::PointDistance,
    Operation::PointLineDistance,
    Operation::LineOffset,
    Operation::CurveOffset,
    Operation::Radius,
    Operation::Diameter,
    Operation::Angle,
    Operation::AxisAngle,
];

#[derive(Debug, Clone, Copy)]
enum PublicDimensionCase {
    LineLength,
    PointDistance,
    PointLineDistance,
    LineAngle,
    LineOffset,
    Diameter,
    Radius,
}

const PUBLIC_DIMENSION_CASES: [PublicDimensionCase; 7] = [
    PublicDimensionCase::LineLength,
    PublicDimensionCase::PointDistance,
    PublicDimensionCase::PointLineDistance,
    PublicDimensionCase::LineAngle,
    PublicDimensionCase::LineOffset,
    PublicDimensionCase::Diameter,
    PublicDimensionCase::Radius,
];

#[derive(Debug, Clone, Copy)]
struct Fixture {
    line_a: EntityId,
    line_b: EntityId,
    point_a: EntityId,
    point_b: EntityId,
    circle_a: EntityId,
    circle_b: EntityId,
    axis: EntityId,
}

#[derive(Debug, Clone, Copy)]
struct DimensionFixture {
    line_a: EntityId,
    line_b: EntityId,
    point_a: EntityId,
    point_b: EntityId,
    parallel_a: EntityId,
    parallel_b: EntityId,
    circle: EntityId,
    arc: EntityId,
}

impl DimensionFixture {
    fn spec(self, case: PublicDimensionCase, dto: &SketchDto) -> (Vec<EntityId>, Constraint) {
        match case {
            PublicDimensionCase::LineLength => {
                let (start, end) = line(dto, self.line_a);
                (
                    vec![self.line_a],
                    Constraint::Distance {
                        from: self.line_a,
                        to: None,
                        value: start.distance(end),
                    },
                )
            }
            PublicDimensionCase::PointDistance => (
                vec![self.point_a, self.point_b],
                Constraint::Distance {
                    from: self.point_a,
                    to: Some(self.point_b),
                    value: point(dto, self.point_a).distance(point(dto, self.point_b)),
                },
            ),
            PublicDimensionCase::PointLineDistance => {
                let (start, end) = line(dto, self.line_b);
                (
                    vec![self.point_a, self.line_b],
                    Constraint::Distance {
                        from: self.point_a,
                        to: Some(self.line_b),
                        value: line_distance(point(dto, self.point_a), start, end),
                    },
                )
            }
            PublicDimensionCase::LineAngle => {
                let (a0, a1) = line(dto, self.line_a);
                let (b0, b1) = line(dto, self.line_b);
                (
                    vec![self.line_a, self.line_b],
                    Constraint::Angle {
                        a: self.line_a,
                        b: self.line_b,
                        value: directed_angle(a1 - a0, b1 - b0).to_degrees(),
                    },
                )
            }
            PublicDimensionCase::LineOffset => {
                let (a0, a1) = line(dto, self.parallel_a);
                let (b0, _) = line(dto, self.parallel_b);
                (
                    vec![self.parallel_a, self.parallel_b],
                    Constraint::Distance {
                        from: self.parallel_a,
                        to: Some(self.parallel_b),
                        value: line_distance(b0, a0, a1),
                    },
                )
            }
            PublicDimensionCase::Diameter => (
                vec![self.circle],
                Constraint::Diameter {
                    entity: self.circle,
                    value: curve(dto, self.circle).1 * 2.0,
                },
            ),
            PublicDimensionCase::Radius => (
                vec![self.arc],
                Constraint::Radius {
                    entity: self.arc,
                    value: curve(dto, self.arc).1,
                },
            ),
        }
    }
}

impl Fixture {
    fn constraint(self, operation: Operation) -> Constraint {
        match operation {
            Operation::Horizontal => Constraint::Horizontal {
                entity: self.line_a,
            },
            Operation::Vertical => Constraint::Vertical {
                entity: self.line_a,
            },
            Operation::HorizontalPoints => Constraint::HorizontalPoints {
                a: self.point_a,
                b: self.point_b,
            },
            Operation::VerticalPoints => Constraint::VerticalPoints {
                a: self.point_a,
                b: self.point_b,
            },
            Operation::Coincident => Constraint::Coincident {
                a: self.point_a,
                b: self.line_b,
            },
            Operation::Tangent => Constraint::Tangent {
                a: self.line_a,
                b: self.circle_a,
            },
            Operation::EqualLines => Constraint::Equal {
                a: self.line_a,
                b: self.line_b,
            },
            Operation::EqualCurves => Constraint::Equal {
                a: self.circle_a,
                b: self.circle_b,
            },
            Operation::Parallel => Constraint::Parallel {
                a: self.line_a,
                b: self.line_b,
            },
            Operation::Perpendicular => Constraint::Perpendicular {
                a: self.line_a,
                b: self.line_b,
            },
            Operation::Fix => Constraint::Fix {
                entity: self.line_a,
            },
            Operation::Midpoint => Constraint::Midpoint {
                a: self.point_a,
                b: self.line_b,
            },
            Operation::Concentric => Constraint::Concentric {
                a: self.circle_a,
                b: self.circle_b,
            },
            Operation::Collinear => Constraint::Collinear {
                a: self.line_a,
                b: self.line_b,
            },
            Operation::SymmetryPoints => Constraint::Symmetry {
                a: self.point_a,
                b: self.point_b,
                axis: self.axis,
            },
            Operation::SymmetryLines => Constraint::Symmetry {
                a: self.line_a,
                b: self.line_b,
                axis: self.axis,
            },
            Operation::LineLength => Constraint::Distance {
                from: self.line_a,
                to: None,
                value: 52.0,
            },
            Operation::PointDistance => Constraint::Distance {
                from: self.point_a,
                to: Some(self.point_b),
                value: 31.0,
            },
            Operation::PointLineDistance => Constraint::Distance {
                from: self.point_a,
                to: Some(self.line_b),
                value: 14.0,
            },
            Operation::LineOffset => Constraint::Distance {
                from: self.line_a,
                to: Some(self.line_b),
                value: 18.0,
            },
            Operation::CurveOffset => Constraint::Distance {
                from: self.circle_a,
                to: Some(self.circle_b),
                value: 3.0,
            },
            Operation::Radius => Constraint::Radius {
                entity: self.circle_a,
                value: 8.0,
            },
            Operation::Diameter => Constraint::Diameter {
                entity: self.circle_a,
                value: 16.0,
            },
            Operation::Angle => Constraint::Angle {
                a: self.line_a,
                b: self.line_b,
                value: 35.0,
            },
            Operation::AxisAngle => Constraint::Angle {
                a: self.line_a,
                b: EntityId(0),
                value: 20.0,
            },
        }
    }
}

fn fixture() -> (SketchSession, Fixture) {
    let mut session = SketchSession::new("pair matrix", XY, XY.basis().unwrap(), false);
    let fixture = add_fixture(&mut session, Vec2::ZERO);
    (session, fixture)
}

fn add_fixture(session: &mut SketchSession, offset: Vec2) -> Fixture {
    let line_a = session
        .add_line(offset + v(0.0, 0.0), offset + v(40.0, 10.0), true)
        .expect("line A");
    let line_b = session
        .add_line(offset + v(15.0, 30.0), offset + v(48.0, 53.0), true)
        .expect("line B");
    let axis = session
        .add_line(offset + v(-20.0, 18.0), offset + v(65.0, 18.0), true)
        .expect("symmetry axis");
    let circle_a = session
        .add_circle(
            CircleMode::CenterDiameter,
            offset + v(25.0, 18.0),
            offset + v(31.5, 18.0),
        )
        .expect("circle A")
        .entities[0];
    let circle_b = session
        .add_circle(
            CircleMode::CenterDiameter,
            offset + v(48.0, 32.0),
            offset + v(58.0, 32.0),
        )
        .expect("circle B")
        .entities[0];

    Fixture {
        line_a: line_a.entity_id,
        line_b: line_b.entity_id,
        point_a: line_a.end_point_id,
        point_b: line_b.start_point_id,
        circle_a,
        circle_b,
        axis: axis.entity_id,
    }
}

fn dimension_fixture() -> (SketchSession, DimensionFixture) {
    let mut session = SketchSession::new("public dimension order", XY, XY.basis().unwrap(), false);
    let line_a = session
        .add_line(v(0.0, 0.0), v(40.0, 10.0), true)
        .expect("dimension line A");
    let line_b = session
        .add_line(v(15.0, 30.0), v(48.0, 53.0), true)
        .expect("dimension line B");
    let parallel_a = session
        .add_line(v(-10.0, -45.0), v(30.0, -35.0), true)
        .expect("parallel dimension line A");
    let parallel_b = session
        .add_line(v(0.0, -10.0), v(40.0, 0.0), true)
        .expect("parallel dimension line B");
    let circle = session
        .add_circle(CircleMode::CenterDiameter, v(75.0, 20.0), v(82.0, 20.0))
        .expect("dimension circle")
        .entities[0];
    let arc = session
        .add_arc_center(v(100.0, 20.0), v(109.0, 20.0), v(100.0, 29.0))
        .expect("dimension arc")
        .entities[0];

    (
        session,
        DimensionFixture {
            line_a: line_a.entity_id,
            line_b: line_b.entity_id,
            point_a: line_a.end_point_id,
            point_b: line_b.start_point_id,
            parallel_a: parallel_a.entity_id,
            parallel_b: parallel_b.entity_id,
            circle,
            arc,
        },
    )
}

fn selected_entities(dto: &SketchDto, ids: &BTreeSet<EntityId>) -> Vec<EntityDto> {
    dto.entities
        .iter()
        .filter(|entity| ids.contains(&entity.id()))
        .cloned()
        .collect()
}

fn line(dto: &SketchDto, id: EntityId) -> (Vec2, Vec2) {
    match dto.entities.iter().find(|entity| entity.id() == id) {
        Some(EntityDto::Line { start, end, .. }) => (*start, *end),
        other => panic!("expected line {id:?}, got {other:?}"),
    }
}

fn point(dto: &SketchDto, id: EntityId) -> Vec2 {
    match dto.entities.iter().find(|entity| entity.id() == id) {
        Some(EntityDto::Point { position, .. }) => *position,
        other => panic!("expected point {id:?}, got {other:?}"),
    }
}

fn curve(dto: &SketchDto, id: EntityId) -> (Vec2, f64) {
    match dto.entities.iter().find(|entity| entity.id() == id) {
        Some(EntityDto::Circle { center, radius, .. })
        | Some(EntityDto::Arc { center, radius, .. }) => (*center, *radius),
        other => panic!("expected curve {id:?}, got {other:?}"),
    }
}

fn line_distance(point: Vec2, start: Vec2, end: Vec2) -> f64 {
    let direction = end - start;
    let relative = point - start;
    (direction.x * relative.y - direction.y * relative.x) / direction.length()
}

fn angle_delta(a: f64, b: f64) -> f64 {
    (a - b + PI).rem_euclid(2.0 * PI) - PI
}

fn directed_angle(a: Vec2, b: Vec2) -> f64 {
    (a.x * b.y - a.y * b.x).atan2(a.dot(b)).rem_euclid(2.0 * PI)
}

fn reflect_across_line(point: Vec2, axis_start: Vec2, axis_end: Vec2) -> Vec2 {
    let tangent = (axis_end - axis_start) * (1.0 / axis_start.distance(axis_end));
    let relative = point - axis_start;
    let projected = axis_start + tangent * relative.dot(tangent);
    projected * 2.0 - point
}

fn assert_constraint_holds(dto: &SketchDto, constraint: Constraint, context: &str) {
    let residual = match constraint {
        Constraint::Horizontal { entity } => {
            let (a, b) = line(dto, entity);
            (b.y - a.y).abs()
        }
        Constraint::Vertical { entity } => {
            let (a, b) = line(dto, entity);
            (b.x - a.x).abs()
        }
        Constraint::HorizontalPoints { a, b } => (point(dto, b).y - point(dto, a).y).abs(),
        Constraint::VerticalPoints { a, b } => (point(dto, b).x - point(dto, a).x).abs(),
        Constraint::Coincident { a, b } => {
            let a_entity = dto.entities.iter().find(|entity| entity.id() == a).unwrap();
            let b_entity = dto.entities.iter().find(|entity| entity.id() == b).unwrap();
            match (a_entity, b_entity) {
                (EntityDto::Point { position, .. }, EntityDto::Point { position: q, .. }) => {
                    position.distance(*q)
                }
                (EntityDto::Point { position, .. }, EntityDto::Line { start, end, .. })
                | (EntityDto::Line { start, end, .. }, EntityDto::Point { position, .. }) => {
                    line_distance(*position, *start, *end).abs()
                }
                (EntityDto::Point { position, .. }, EntityDto::Circle { center, radius, .. })
                | (EntityDto::Circle { center, radius, .. }, EntityDto::Point { position, .. })
                | (EntityDto::Point { position, .. }, EntityDto::Arc { center, radius, .. })
                | (EntityDto::Arc { center, radius, .. }, EntityDto::Point { position, .. }) => {
                    (position.distance(*center) - radius).abs()
                }
                (
                    EntityDto::Circle { center, .. } | EntityDto::Arc { center, .. },
                    EntityDto::Circle { center: other, .. } | EntityDto::Arc { center: other, .. },
                ) => center.distance(*other),
                pair => panic!("unsupported coincident pair {pair:?}"),
            }
        }
        Constraint::Tangent { a, b } => {
            let (line_id, curve_id) = if matches!(
                dto.entities.iter().find(|entity| entity.id() == a),
                Some(EntityDto::Line { .. })
            ) {
                (a, b)
            } else {
                (b, a)
            };
            let (start, end) = line(dto, line_id);
            let (center, radius) = curve(dto, curve_id);
            (line_distance(center, start, end).abs() - radius).abs()
        }
        Constraint::Equal { a, b } => {
            if matches!(
                dto.entities.iter().find(|entity| entity.id() == a),
                Some(EntityDto::Line { .. })
            ) {
                let (a0, a1) = line(dto, a);
                let (b0, b1) = line(dto, b);
                (a0.distance(a1) - b0.distance(b1)).abs()
            } else {
                (curve(dto, a).1 - curve(dto, b).1).abs()
            }
        }
        Constraint::Parallel { a, b } | Constraint::Collinear { a, b } => {
            let (a0, a1) = line(dto, a);
            let (b0, b1) = line(dto, b);
            let da = a1 - a0;
            let db = b1 - b0;
            let direction = (da.x * db.y - da.y * db.x).abs() / (da.length() * db.length());
            if matches!(constraint, Constraint::Collinear { .. }) {
                direction.max(line_distance(b0, a0, a1).abs())
            } else {
                direction
            }
        }
        Constraint::Perpendicular { a, b } => {
            let (a0, a1) = line(dto, a);
            let (b0, b1) = line(dto, b);
            ((a1 - a0).dot(b1 - b0)).abs() / (a0.distance(a1) * b0.distance(b1))
        }
        Constraint::Fix { .. } => 0.0,
        Constraint::Midpoint { a, b } => {
            let (start, end) = line(dto, b);
            point(dto, a).distance((start + end) * 0.5)
        }
        Constraint::Concentric { a, b } => curve(dto, a).0.distance(curve(dto, b).0),
        Constraint::Symmetry { a, b, axis } => {
            let (axis_start, axis_end) = line(dto, axis);
            if matches!(
                dto.entities.iter().find(|entity| entity.id() == a),
                Some(EntityDto::Point { .. })
            ) {
                reflect_across_line(point(dto, a), axis_start, axis_end).distance(point(dto, b))
            } else {
                let (a0, a1) = line(dto, a);
                let (b0, b1) = line(dto, b);
                reflect_across_line(a0, axis_start, axis_end)
                    .distance(b0)
                    .max(reflect_across_line(a1, axis_start, axis_end).distance(b1))
            }
        }
        Constraint::Distance { from, to, value } => match to {
            None => {
                let (a, b) = line(dto, from);
                (a.distance(b) - value).abs()
            }
            Some(to)
                if matches!(
                    dto.entities.iter().find(|entity| entity.id() == from),
                    Some(EntityDto::Point { .. })
                ) && matches!(
                    dto.entities.iter().find(|entity| entity.id() == to),
                    Some(EntityDto::Point { .. })
                ) =>
            {
                (point(dto, from).distance(point(dto, to)) - value).abs()
            }
            Some(to)
                if matches!(
                    dto.entities.iter().find(|entity| entity.id() == from),
                    Some(EntityDto::Point { .. })
                ) =>
            {
                let (start, end) = line(dto, to);
                (line_distance(point(dto, from), start, end) - value).abs()
            }
            Some(to)
                if matches!(
                    dto.entities.iter().find(|entity| entity.id() == from),
                    Some(EntityDto::Line { .. })
                ) && matches!(
                    dto.entities.iter().find(|entity| entity.id() == to),
                    Some(EntityDto::Line { .. })
                ) =>
            {
                let (a0, a1) = line(dto, from);
                let (b0, _) = line(dto, to);
                (line_distance(b0, a0, a1).abs() - value.abs()).abs()
            }
            Some(to) => ((curve(dto, to).1 - curve(dto, from).1) - value).abs(),
        },
        Constraint::Radius { entity, value } => (curve(dto, entity).1 - value).abs(),
        Constraint::Diameter { entity, value } => (curve(dto, entity).1 * 2.0 - value).abs(),
        Constraint::Angle { a, b, value } => {
            let (a0, a1) = line(dto, a);
            let direction_a = a1 - a0;
            let actual = if b == EntityId(0) {
                direction_a.y.atan2(direction_a.x).rem_euclid(2.0 * PI)
            } else {
                let (b0, b1) = line(dto, b);
                directed_angle(direction_a, b1 - b0)
            };
            angle_delta(actual, value.to_radians()).abs()
        }
        other => panic!("internal constraint escaped the panel matrix: {other:?}"),
    };
    assert!(
        residual <= EPS,
        "{context}: {constraint:?} residual={residual}"
    );
}

fn assert_bounded(dto: &SketchDto, context: &str) {
    for entity in &dto.entities {
        match entity {
            EntityDto::Point { position, .. } => {
                assert!(
                    position.x.is_finite() && position.y.is_finite(),
                    "{context}: {entity:?}"
                );
                assert!(
                    position.x.abs() < 10_000.0 && position.y.abs() < 10_000.0,
                    "{context}: {entity:?}"
                );
            }
            EntityDto::Line { start, end, .. } => {
                let length = start.distance(*end);
                assert!(
                    length.is_finite() && (1.0e-6..10_000.0).contains(&length),
                    "{context}: {entity:?}"
                );
            }
            EntityDto::Circle { center, radius, .. } | EntityDto::Arc { center, radius, .. } => {
                assert!(
                    center.x.is_finite() && center.y.is_finite(),
                    "{context}: {entity:?}"
                );
                assert!(
                    radius.is_finite() && (1.0e-6..10_000.0).contains(radius),
                    "{context}: {entity:?}"
                );
            }
            EntityDto::Spline { points, .. } => {
                assert!(
                    points
                        .iter()
                        .all(|point| point.x.is_finite() && point.y.is_finite()),
                    "{context}: {entity:?}"
                );
            }
        }
    }
}

fn assert_vec_unchanged(before: Vec2, after: Vec2, context: &str, property: &str) {
    assert!(
        before.distance(after) <= EPS,
        "{context}: {property} changed from {before:?} to {after:?}"
    );
}

fn assert_scalar_unchanged(before: f64, after: f64, context: &str, property: &str) {
    assert!(
        (before - after).abs() <= EPS,
        "{context}: {property} changed from {before} to {after}"
    );
}

fn assert_line_shape_unchanged(
    before: &SketchDto,
    after: &SketchDto,
    entity: EntityId,
    context: &str,
) {
    let (before_start, before_end) = line(before, entity);
    let (after_start, after_end) = line(after, entity);
    let before_direction = before_end - before_start;
    let after_direction = after_end - after_start;
    assert_scalar_unchanged(
        before_direction.length(),
        after_direction.length(),
        context,
        "line length",
    );
    assert!(
        angle_delta(
            before_direction.y.atan2(before_direction.x),
            after_direction.y.atan2(after_direction.x),
        )
        .abs()
            <= EPS,
        "{context}: line bearing changed from {before_direction:?} to {after_direction:?}"
    );
}

fn assert_line_pose_unchanged(
    before: &SketchDto,
    after: &SketchDto,
    entity: EntityId,
    context: &str,
) {
    assert_line_shape_unchanged(before, after, entity, context);
    let (before_start, before_end) = line(before, entity);
    let (after_start, after_end) = line(after, entity);
    assert_vec_unchanged(
        (before_start + before_end) * 0.5,
        (after_start + after_end) * 0.5,
        context,
        "line midpoint",
    );
}

fn assert_line_length_unchanged(
    before: &SketchDto,
    after: &SketchDto,
    entity: EntityId,
    context: &str,
) {
    let (before_start, before_end) = line(before, entity);
    let (after_start, after_end) = line(after, entity);
    assert_scalar_unchanged(
        before_start.distance(before_end),
        after_start.distance(after_end),
        context,
        "line length",
    );
}

fn assert_operation_invariants(
    before: &SketchDto,
    after: &SketchDto,
    fixture: Fixture,
    first: Operation,
    operation: Operation,
    context: &str,
) {
    match operation {
        Operation::Horizontal | Operation::Vertical | Operation::AxisAngle => {
            assert_line_length_unchanged(before, after, fixture.line_a, context);
        }
        Operation::HorizontalPoints | Operation::VerticalPoints => {
            let complementary_alignment = matches!(
                (first, operation),
                (Operation::HorizontalPoints, Operation::VerticalPoints)
                    | (Operation::VerticalPoints, Operation::HorizontalPoints)
            );
            if complementary_alignment {
                return;
            }
            let before_a = point(before, fixture.point_a);
            let before_b = point(before, fixture.point_b);
            let after_a = point(after, fixture.point_a);
            let after_b = point(after, fixture.point_b);
            assert_scalar_unchanged(
                before_a.distance(before_b),
                after_a.distance(after_b),
                context,
                "point-pair distance",
            );
        }
        Operation::Coincident | Operation::Midpoint | Operation::PointLineDistance => {
            if !(first == Operation::PointDistance && operation == Operation::Midpoint) {
                assert_line_length_unchanged(before, after, fixture.line_b, context);
            }
        }
        Operation::Tangent => {
            assert_line_length_unchanged(before, after, fixture.line_a, context);
            assert_scalar_unchanged(
                curve(before, fixture.circle_a).1,
                curve(after, fixture.circle_a).1,
                context,
                "tangent curve radius",
            );
        }
        Operation::EqualLines => {
            assert_line_shape_unchanged(before, after, fixture.line_a, context);
            let (before_start, before_end) = line(before, fixture.line_b);
            let (after_start, after_end) = line(after, fixture.line_b);
            assert!(
                angle_delta(
                    (before_end - before_start)
                        .y
                        .atan2((before_end - before_start).x),
                    (after_end - after_start)
                        .y
                        .atan2((after_end - after_start).x),
                )
                .abs()
                    <= EPS,
                "{context}: Equal changed the target bearing"
            );
        }
        Operation::EqualCurves => {
            let (_, before_reference_radius) = curve(before, fixture.circle_a);
            let (_, after_reference_radius) = curve(after, fixture.circle_a);
            assert_scalar_unchanged(
                before_reference_radius,
                after_reference_radius,
                context,
                "equal reference radius",
            );
        }
        Operation::Parallel | Operation::Perpendicular | Operation::Angle => {
            assert_line_length_unchanged(before, after, fixture.line_a, context);
            assert_line_length_unchanged(before, after, fixture.line_b, context);
        }
        Operation::Fix => {
            assert_line_pose_unchanged(before, after, fixture.line_a, context);
        }
        Operation::Concentric => {
            let (_, before_ra) = curve(before, fixture.circle_a);
            let (_, after_ra) = curve(after, fixture.circle_a);
            assert_scalar_unchanged(before_ra, after_ra, context, "concentric reference radius");
            assert_scalar_unchanged(
                curve(before, fixture.circle_b).1,
                curve(after, fixture.circle_b).1,
                context,
                "concentric target radius",
            );
        }
        Operation::Collinear => {
            assert_line_shape_unchanged(before, after, fixture.line_a, context);
            let (before_b0, before_b1) = line(before, fixture.line_b);
            let (after_b0, after_b1) = line(after, fixture.line_b);
            assert_scalar_unchanged(
                before_b0.distance(before_b1),
                after_b0.distance(after_b1),
                context,
                "collinear target length",
            );
        }
        Operation::SymmetryPoints | Operation::SymmetryLines => {
            assert_line_pose_unchanged(before, after, fixture.axis, context);
            if operation == Operation::SymmetryLines {
                let (before_start, before_end) = line(before, fixture.line_a);
                let (after_start, after_end) = line(after, fixture.line_a);
                assert_scalar_unchanged(
                    before_start.distance(before_end),
                    after_start.distance(after_end),
                    context,
                    "symmetry reference length",
                );
            }
        }
        Operation::LineLength => {
            let (before_start, before_end) = line(before, fixture.line_a);
            let (after_start, after_end) = line(after, fixture.line_a);
            assert!(
                angle_delta(
                    (before_end - before_start)
                        .y
                        .atan2((before_end - before_start).x),
                    (after_end - after_start)
                        .y
                        .atan2((after_end - after_start).x),
                )
                .abs()
                    <= EPS,
                "{context}: length dimension changed bearing"
            );
        }
        Operation::PointDistance => {
            let before_direction = point(before, fixture.point_b) - point(before, fixture.point_a);
            let after_direction = point(after, fixture.point_b) - point(after, fixture.point_a);
            assert!(
                angle_delta(
                    before_direction.y.atan2(before_direction.x),
                    after_direction.y.atan2(after_direction.x),
                )
                .abs()
                    <= EPS,
                "{context}: point distance changed bearing"
            );
        }
        Operation::LineOffset => {
            assert_line_length_unchanged(before, after, fixture.line_a, context);
            assert_line_length_unchanged(before, after, fixture.line_b, context);
        }
        Operation::CurveOffset => {
            let (_, before_radius) = curve(before, fixture.circle_a);
            assert_scalar_unchanged(
                before_radius,
                curve(after, fixture.circle_a).1,
                context,
                "radial-offset reference radius",
            );
        }
        Operation::Radius | Operation::Diameter => {
            assert_vec_unchanged(
                curve(before, fixture.circle_a).0,
                curve(after, fixture.circle_a).0,
                context,
                "radial-dimension center",
            );
        }
    }
}

#[test]
fn every_public_constraint_solves_in_isolation() {
    for operation in OPERATIONS {
        let (mut session, fixture) = fixture();
        let constraint = fixture.constraint(operation);
        let result = session.add_constraint(constraint);
        assert!(result.is_ok(), "{operation:?} failed alone: {result:?}");
        let dto = session.dto();
        assert_constraint_holds(&dto, constraint, &format!("{operation:?}"));
        assert_bounded(&dto, &format!("{operation:?}"));
    }
}

#[test]
fn every_ordered_constraint_pair_is_exact_or_rejected_atomically() {
    let mut successes = 0usize;
    let mut atomic_rejections = 0usize;
    let mut failures = Vec::new();

    for first in OPERATIONS {
        for second in OPERATIONS {
            let outcome = std::panic::catch_unwind(|| {
                let context = format!("{first:?} -> {second:?}");
                let (mut session, fixture) = fixture();
                let first_constraint = fixture.constraint(first);
                session
                    .add_constraint(first_constraint)
                    .unwrap_or_else(|error| panic!("{context}: first operation failed: {error}"));
                let before_second = session.dto();
                let second_constraint = fixture.constraint(second);

                match session.add_constraint(second_constraint) {
                    Ok(_) => {
                        let after = session.dto();
                        assert_constraint_holds(&after, first_constraint, &context);
                        assert_constraint_holds(&after, second_constraint, &context);
                        assert_operation_invariants(
                            &before_second,
                            &after,
                            fixture,
                            first,
                            second,
                            &context,
                        );
                        assert_bounded(&after, &context);
                        true
                    }
                    Err(_) => {
                        let after = session.dto();
                        assert_eq!(
                            after.entities, before_second.entities,
                            "{context}: rejected constraint changed geometry"
                        );
                        assert_eq!(
                            after.constraints, before_second.constraints,
                            "{context}: rejected constraint changed constraints"
                        );
                        assert_constraint_holds(&after, first_constraint, &context);
                        assert_bounded(&after, &context);
                        false
                    }
                }
            });
            match outcome {
                Ok(true) => successes += 1,
                Ok(false) => atomic_rejections += 1,
                Err(payload) => {
                    let message = payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| {
                            payload
                                .downcast_ref::<&str>()
                                .map(|value| (*value).to_string())
                        })
                        .unwrap_or_else(|| "unknown panic".to_string());
                    failures.push(format!("{first:?} -> {second:?}: {message}"));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} pair failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(successes + atomic_rejections, OPERATIONS.len().pow(2));
}

#[test]
fn every_ordered_pair_leaves_a_disconnected_solved_island_untouched() {
    for first in OPERATIONS {
        for second in OPERATIONS {
            let context = format!("disconnected {first:?} -> {second:?}");
            let mut session =
                SketchSession::new("disconnected pair matrix", XY, XY.basis().unwrap(), false);
            let first_fixture = add_fixture(&mut session, Vec2::ZERO);
            let first_ids = session
                .dto()
                .entities
                .iter()
                .map(EntityDto::id)
                .collect::<BTreeSet<_>>();
            let second_fixture = add_fixture(&mut session, v(400.0, -250.0));

            let first_constraint = first_fixture.constraint(first);
            session
                .add_constraint(first_constraint)
                .unwrap_or_else(|error| panic!("{context}: first operation failed: {error}"));
            let before_second = selected_entities(&session.dto(), &first_ids);

            let second_constraint = second_fixture.constraint(second);
            session
                .add_constraint(second_constraint)
                .unwrap_or_else(|error| panic!("{context}: second operation failed: {error}"));
            let after = session.dto();
            assert_eq!(
                selected_entities(&after, &first_ids),
                before_second,
                "{context}: solving the second island moved the first"
            );
            assert_constraint_holds(&after, first_constraint, &context);
            assert_constraint_holds(&after, second_constraint, &context);
            assert_bounded(&after, &context);
        }
    }
}

#[test]
fn public_dimension_commands_are_order_safe_and_become_reference_when_already_driven() {
    for case in PUBLIC_DIMENSION_CASES {
        let context = format!("public {case:?}");

        let (mut relation_first, fixture) = dimension_fixture();
        let (entities, relation) = fixture.spec(case, &relation_first.dto());
        relation_first
            .add_constraint(relation)
            .unwrap_or_else(|error| panic!("{context}: relation failed: {error}"));
        let before_reference = relation_first.dto();
        let after_reference = relation_first
            .add_dimension(DimensionRequest {
                entities: entities.clone(),
                text_pos: v(12.0, 8.0),
                value_text: None,
            })
            .unwrap_or_else(|error| panic!("{context}: reference dimension failed: {error}"))
            .sketch;
        assert_eq!(
            after_reference.entities, before_reference.entities,
            "{context}: adding a reference dimension moved geometry"
        );
        assert_eq!(after_reference.dimensions.len(), 1, "{context}");
        assert_eq!(
            after_reference.dimensions[0].mode,
            DimensionMode::Reference,
            "{context}: an already-driven measurement must be reference"
        );
        assert_constraint_holds(&after_reference, relation, &context);
        assert_bounded(&after_reference, &context);

        let (mut typed_reference, fixture) = dimension_fixture();
        let (entities, relation) = fixture.spec(case, &typed_reference.dto());
        typed_reference
            .add_constraint(relation)
            .unwrap_or_else(|error| panic!("{context}: typed setup failed: {error}"));
        let before_typed = typed_reference.dto();
        assert!(
            typed_reference
                .add_dimension(DimensionRequest {
                    entities,
                    text_pos: v(12.0, 8.0),
                    value_text: Some("123".to_string()),
                })
                .is_err(),
            "{context}: an already-driven measurement accepted a target value"
        );
        assert_eq!(
            typed_reference.dto(),
            before_typed,
            "{context}: rejected target changed the sketch"
        );

        let (mut dimension_first, fixture) = dimension_fixture();
        let (entities, relation) = fixture.spec(case, &dimension_first.dto());
        let driven = dimension_first
            .add_dimension(DimensionRequest {
                entities,
                text_pos: v(12.0, 8.0),
                value_text: None,
            })
            .unwrap_or_else(|error| panic!("{context}: driving dimension failed: {error}"))
            .sketch;
        assert_eq!(driven.dimensions.len(), 1, "{context}");
        assert_eq!(
            driven.dimensions[0].mode,
            DimensionMode::Driving,
            "{context}: the first measurement should drive"
        );
        let before_duplicate = dimension_first.dto();
        assert!(
            dimension_first.add_constraint(relation).is_err(),
            "{context}: equivalent relation was accepted after its dimension"
        );
        assert_eq!(
            dimension_first.dto(),
            before_duplicate,
            "{context}: rejected equivalent relation changed the sketch"
        );
    }
}

#[test]
fn key_order_reversals_reach_the_same_owned_measurements() {
    let pairs = [
        (Operation::Parallel, Operation::EqualLines),
        (Operation::Perpendicular, Operation::EqualLines),
        (Operation::Horizontal, Operation::LineLength),
        (Operation::Parallel, Operation::LineLength),
        (Operation::Concentric, Operation::EqualCurves),
        (Operation::Concentric, Operation::Radius),
        (Operation::Tangent, Operation::Radius),
        (Operation::HorizontalPoints, Operation::PointDistance),
    ];

    for (first, second) in pairs {
        let solve = |order: [Operation; 2]| {
            let (mut session, fixture) = fixture();
            for operation in order {
                session
                    .add_constraint(fixture.constraint(operation))
                    .unwrap_or_else(|error| panic!("{order:?}: {error}"));
            }
            session.dto()
        };
        let forward = solve([first, second]);
        let reverse = solve([second, first]);
        for operation in [first, second] {
            assert_constraint_holds(
                &forward,
                fixture().1.constraint(operation),
                &format!("{first:?} -> {second:?}"),
            );
            assert_constraint_holds(
                &reverse,
                fixture().1.constraint(operation),
                &format!("{second:?} -> {first:?}"),
            );
        }
    }
}
