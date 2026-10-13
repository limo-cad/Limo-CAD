//! Screen-space sketch acquisition. Picking follows the rendered plane and
//! camera, with a fixed logical-pixel tolerance at every zoom level.
use limo_cad_sketch::{EntityDto, EntityId, Vec2};

fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0] * d[0] + d[1] * d[1];
    let t = if length > f32::EPSILON {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length).clamp(0., 1.)
    } else {
        0.
    };
    (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
}

pub(crate) fn hit(
    entities: &[EntityDto],
    cursor: [f32; 2],
    points: bool,
    mut project: impl FnMut(Vec2) -> Option<[f32; 2]>,
) -> Option<EntityId> {
    let mut best: Option<(bool, f32, EntityId)> = None;
    for entity in entities {
        let is_point = matches!(entity, EntityDto::Point { .. });
        if is_point && !points {
            continue;
        }
        let mut distance = f32::INFINITY;
        let mut line = |a, b| {
            if let (Some(a), Some(b)) = (project(a), project(b)) {
                distance = distance.min(segment_distance(cursor, a, b));
            }
        };
        match entity {
            EntityDto::Point { position, .. } => line(*position, *position),
            EntityDto::Line {
                start,
                end,
                consumed: false,
                ..
            } => line(*start, *end),
            EntityDto::Spline { tessellation, .. } => {
                for pair in tessellation.windows(2) {
                    line(pair[0], pair[1]);
                }
            }
            EntityDto::Circle { center, radius, .. } | EntityDto::Arc { center, radius, .. } => {
                let (start, sweep) = match entity {
                    EntityDto::Arc {
                        start_angle,
                        end_angle,
                        ..
                    } => (
                        *start_angle,
                        (end_angle - start_angle).rem_euclid(std::f64::consts::TAU),
                    ),
                    _ => (0., std::f64::consts::TAU),
                };
                let radius_px = match (
                    project(*center),
                    project(Vec2::new(center.x + radius, center.y)),
                    project(Vec2::new(center.x, center.y + radius)),
                ) {
                    (Some(c), Some(x), Some(y)) => (x[0] - c[0])
                        .hypot(x[1] - c[1])
                        .max((y[0] - c[0]).hypot(y[1] - c[1])),
                    _ => 100.,
                };
                let count =
                    ((sweep * f64::from(radius_px).sqrt() * 2.).ceil() as usize).clamp(24, 4096);
                let at = |angle: f64| {
                    Vec2::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    )
                };
                for i in 0..count {
                    if let (Some(a), Some(b)) = (
                        project(at(start + sweep * i as f64 / count as f64)),
                        project(at(start + sweep * (i + 1) as f64 / count as f64)),
                    ) {
                        distance = distance.min(segment_distance(cursor, a, b));
                    }
                }
            }
            _ => {}
        }
        if distance > 7. {
            continue;
        }
        if best.is_none_or(|(point, old, id)| {
            is_point && !point
                || is_point == point && (distance < old || distance == old && entity.id().0 < id.0)
        }) {
            best = Some((is_point, distance, entity.id()));
        }
    }
    best.map(|(_, _, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_priority_consumed_lines_and_zoom_are_respected() {
        let entities = vec![
            EntityDto::Point {
                id: EntityId(1),
                position: Vec2::new(10., 10.),
                fully_defined: false,
            },
            EntityDto::Line {
                id: EntityId(2),
                start_id: EntityId(1),
                end_id: EntityId(3),
                start: Vec2::new(10., 10.),
                end: Vec2::new(20., 10.),
                consumed: false,
                fully_defined: false,
            },
            EntityDto::Line {
                id: EntityId(4),
                start_id: EntityId(1),
                end_id: EntityId(3),
                start: Vec2::new(10., 20.),
                end: Vec2::new(20., 20.),
                consumed: true,
                fully_defined: false,
            },
        ];
        let project = |p: Vec2| Some([p.x as f32 * 10., p.y as f32 * 10.]);
        assert_eq!(
            hit(&entities, [103., 102.], true, project),
            Some(EntityId(1))
        );
        assert_eq!(
            hit(&entities, [103., 102.], false, project),
            Some(EntityId(2))
        );
        assert_eq!(
            hit(&entities, [150., 106.], true, project),
            Some(EntityId(2))
        );
        assert_eq!(hit(&entities, [150., 108.], true, project), None);
        assert_eq!(hit(&entities, [150., 200.], true, project), None);
    }
    #[test]
    fn arc_selection_never_hits_the_missing_half() {
        let entities = [EntityDto::Arc {
            id: EntityId(1),
            center: Vec2::ZERO,
            radius: 1000.,
            start_angle: 0.,
            end_angle: std::f64::consts::PI,
            fully_defined: false,
        }];
        let project = |p: Vec2| Some([p.x as f32, p.y as f32]);
        assert_eq!(
            hit(&entities, [0., 1000.], true, project),
            Some(EntityId(1))
        );
        assert_eq!(hit(&entities, [0., -1000.], true, project), None);
    }
}
