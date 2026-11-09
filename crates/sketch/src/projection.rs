//! Associative, owner-scoped sketch references. Authored identities persist;
//! geometry is re-evaluated from the source and never falls back to stale data.
use crate::{Entity, EntityId, Sketch, Vec2};
use limo_cad_core::{FeatureId, PlaneBasis};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SketchCurveRef {
    pub feature: FeatureId,
    pub entity: EntityId,
}

/// Authored association. Coordinates are derived from the source, never used
/// as a fallback when it disappears. Identity is local to the target sketch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionBinding {
    pub id: u32,
    pub source: SketchCurveRef,
}

pub const MAX_PROJECTIONS: usize = 128;
pub(crate) const MAX_PROJECTION_ID: u32 = 65_536;

impl ProjectionBinding {
    pub(crate) fn edge_id(&self) -> limo_cad_core::EdgeId {
        // Keep public numeric references exactly representable by JSON clients.
        // Collisions with real support edges are rejected by the manager.
        limo_cad_core::EdgeId((1 << 50) + u64::from(self.id))
    }

    pub(crate) fn curve_id(&self) -> u64 {
        (1 << 41) + u64::from(self.id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCurvesRequest {
    pub sources: Vec<SketchCurveRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedLine {
    pub source: SketchCurveRef,
    pub start: Vec2,
    pub end: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionError {
    WrongSourceOwner,
    SelfReference,
    MissingSource,
    UnsupportedCurve,
    MissingEndpoint,
    InvalidBasis,
    NonFiniteCoordinates,
    CollapsedLine,
}

fn valid_basis(b: &PlaneBasis) -> bool {
    let dot = |a: [f64; 3], c: [f64; 3]| a.iter().zip(c).map(|(a, c)| a * c).sum::<f64>();
    b.origin
        .iter()
        .chain(&b.u)
        .chain(&b.v)
        .chain(&b.normal)
        .all(|v| v.is_finite())
        && [b.u, b.v, b.normal]
            .iter()
            .all(|v| (dot(*v, *v) - 1.).abs() <= 1e-9)
        && dot(b.u, b.v).abs() <= 1e-9
        && dot(b.u, b.normal).abs() <= 1e-9
        && dot(b.v, b.normal).abs() <= 1e-9
}

/// Re-evaluate against current source geometry. No stored endpoint fallback is
/// used when the entity or owner disappears. The caller must resolve the source
/// sketch and its basis under a document/revision fence and check dependencies.
pub fn project_sketch_line(
    reference: SketchCurveRef,
    source_feature: FeatureId,
    source: &Sketch,
    source_basis: &PlaneBasis,
    target_feature: FeatureId,
    target_basis: &PlaneBasis,
) -> Result<ProjectedLine, ProjectionError> {
    if reference.feature != source_feature {
        return Err(ProjectionError::WrongSourceOwner);
    }
    if source_feature == target_feature {
        return Err(ProjectionError::SelfReference);
    }
    if !valid_basis(source_basis) || !valid_basis(target_basis) {
        return Err(ProjectionError::InvalidBasis);
    }
    let (start, end) = match source.entity(reference.entity) {
        Some(Entity::Line { start, end }) => (*start, *end),
        Some(_) => return Err(ProjectionError::UnsupportedCurve),
        None => return Err(ProjectionError::MissingSource),
    };
    let project = |id| {
        let Some(Entity::Point { position }) = source.entity(id) else {
            return Err(ProjectionError::MissingEndpoint);
        };
        let world = source_basis.to_3d([position.x, position.y]);
        let projected = target_basis
            .project_point(world)
            .map_err(|_| ProjectionError::NonFiniteCoordinates)?;
        let local = target_basis.to_2d(projected.point);
        if local.iter().any(|v| !v.is_finite()) {
            return Err(ProjectionError::NonFiniteCoordinates);
        }
        Ok(Vec2::new(local[0], local[1]))
    };
    let start = project(start)?;
    let end = project(end)?;
    let length = (end.x - start.x).hypot(end.y - start.y);
    if !length.is_finite() {
        return Err(ProjectionError::NonFiniteCoordinates);
    }
    if length <= 1e-9 {
        return Err(ProjectionError::CollapsedLine);
    }
    Ok(ProjectedLine {
        source: reference,
        start,
        end,
    })
}

/// Project supported authored curves. Circular references retain their exact
/// carrier only on parallel planes; an oblique ellipse is rejected rather than
/// silently changing a circle into an approximate polygon.
pub(crate) fn project_sketch_curve(
    reference: SketchCurveRef,
    source_feature: FeatureId,
    source: &Sketch,
    source_basis: &PlaneBasis,
    target_feature: FeatureId,
    target_basis: &PlaneBasis,
    binding: &ProjectionBinding,
) -> Result<crate::ProjectedEdgeDto, ProjectionError> {
    if reference.feature != source_feature {
        return Err(ProjectionError::WrongSourceOwner);
    }
    if source_feature == target_feature {
        return Err(ProjectionError::SelfReference);
    }
    if !valid_basis(source_basis) || !valid_basis(target_basis) {
        return Err(ProjectionError::InvalidBasis);
    }
    let project = |point: Vec2| -> Result<Vec2, ProjectionError> {
        let world = source_basis.to_3d([point.x, point.y]);
        let local = target_basis.to_2d(
            target_basis
                .project_point(world)
                .map_err(|_| ProjectionError::NonFiniteCoordinates)?
                .point,
        );
        if local.iter().any(|value| !value.is_finite()) {
            return Err(ProjectionError::NonFiniteCoordinates);
        }
        Ok(Vec2::new(local[0], local[1]))
    };
    let mut circle = None;
    let points = match source.entity(reference.entity) {
        Some(Entity::Line { .. }) => {
            let line = project_sketch_line(
                reference,
                source_feature,
                source,
                source_basis,
                target_feature,
                target_basis,
            )?;
            vec![line.start, line.end]
        }
        Some(entity @ (Entity::Circle { .. } | Entity::Arc { .. })) => {
            let (center, radius, start, sweep, closed) = match *entity {
                Entity::Circle { center, radius } => {
                    (center, radius, 0., std::f64::consts::TAU, true)
                }
                Entity::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } => {
                    let span = end_angle - start_angle;
                    let sweep = if span > 0. {
                        span
                    } else {
                        span.rem_euclid(std::f64::consts::TAU)
                    };
                    (center, radius, start_angle, sweep, false)
                }
                _ => unreachable!(),
            };
            let alignment: f64 = source_basis
                .normal
                .iter()
                .zip(target_basis.normal)
                .map(|(a, b)| a * b)
                .sum();
            if (alignment.abs() - 1.).abs() > 1e-9 {
                return Err(ProjectionError::UnsupportedCurve);
            }
            if !radius.is_finite()
                || radius <= 1e-9
                || !start.is_finite()
                || !sweep.is_finite()
                || sweep <= 1e-9
                || sweep > std::f64::consts::TAU + 1e-9
            {
                return Err(ProjectionError::CollapsedLine);
            }
            circle = Some(crate::ProjectedCircleDto {
                center: project(center)?,
                radius,
                closed,
            });
            let segments = ((sweep / std::f64::consts::TAU * 128.).ceil() as usize).clamp(4, 128);
            (0..=segments)
                .map(|index| {
                    let angle = start + sweep * index as f64 / segments as f64;
                    project(Vec2::new(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        Some(Entity::Spline { points }) if (2..=256).contains(&points.len()) => {
            crate::geomops::spline::tessellate_spline(points, 16)
                .into_iter()
                .map(project)
                .collect::<Result<Vec<_>, _>>()?
        }
        Some(_) => return Err(ProjectionError::UnsupportedCurve),
        None => return Err(ProjectionError::MissingSource),
    };
    if points.len() < 2
        || !points
            .windows(2)
            .any(|pair| pair[0].distance(pair[1]) > 1e-9)
    {
        return Err(ProjectionError::CollapsedLine);
    }
    Ok(crate::ProjectedEdgeDto {
        id: binding.curve_id(),
        edge_id: binding.edge_id(),
        points,
        circle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_core::{OriginPlane, PlaneRef};

    fn fixture() -> (Sketch, SketchCurveRef, EntityId, PlaneBasis) {
        let mut sketch = Sketch::new();
        let a = sketch.add_entity(Entity::point(1., 2.));
        let b = sketch.add_entity(Entity::point(5., 7.));
        let line = sketch.add_entity(Entity::line(a, b));
        let basis = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .basis()
        .unwrap();
        (
            sketch,
            SketchCurveRef {
                feature: FeatureId(1),
                entity: line,
            },
            b,
            basis,
        )
    }

    #[test]
    fn translated_rotated_target_projects_into_target_coordinates() {
        let (sketch, reference, _, source) = fixture();
        let target = PlaneBasis {
            origin: [10., 20., 8.],
            u: [0., 1., 0.],
            v: [-1., 0., 0.],
            normal: [0., 0., 1.],
        };
        let result = project_sketch_line(
            reference,
            FeatureId(1),
            &sketch,
            &source,
            FeatureId(2),
            &target,
        )
        .unwrap();
        assert_eq!(result.start, Vec2::new(-18., 9.));
        assert_eq!(result.end, Vec2::new(-13., 5.));
    }

    #[test]
    fn circular_projection_keeps_exact_carrier_and_rejects_oblique_ellipse() {
        let (mut sketch, reference, _, basis) = fixture();
        *sketch.entity_mut(reference.entity).unwrap() = Entity::circle(3., 4., 6.);
        let binding = ProjectionBinding {
            id: 7,
            source: reference,
        };
        let mut target = basis;
        target.origin = [10., 20., 30.];
        target.u = [0., 1., 0.];
        target.v = [-1., 0., 0.];
        let projected = project_sketch_curve(
            reference,
            FeatureId(1),
            &sketch,
            &basis,
            FeatureId(2),
            &target,
            &binding,
        )
        .unwrap();
        assert_eq!(
            projected.circle.unwrap(),
            crate::ProjectedCircleDto {
                center: Vec2::new(-16., 7.),
                radius: 6.,
                closed: true
            }
        );
        assert!(
            projected
                .points
                .first()
                .unwrap()
                .distance(*projected.points.last().unwrap())
                < 1e-12
        );
        let oblique = PlaneRef::OriginPlane {
            plane: OriginPlane::Yz,
        }
        .basis()
        .unwrap();
        assert_eq!(
            project_sketch_curve(
                reference,
                FeatureId(1),
                &sketch,
                &basis,
                FeatureId(2),
                &oblique,
                &binding
            ),
            Err(ProjectionError::UnsupportedCurve)
        );
        *sketch.entity_mut(reference.entity).unwrap() =
            Entity::arc(3., 4., 6., 0., std::f64::consts::FRAC_PI_2);
        let arc = project_sketch_curve(
            reference,
            FeatureId(1),
            &sketch,
            &basis,
            FeatureId(2),
            &basis,
            &binding,
        )
        .unwrap();
        assert!(!arc.circle.unwrap().closed);
        assert!(arc.points[0].distance(Vec2::new(9., 4.)) < 1e-12);
        assert!(arc.points.last().unwrap().distance(Vec2::new(3., 10.)) < 1e-12);
    }

    #[test]
    fn re_resolves_current_source_after_edit_without_changing_it() {
        let (mut sketch, reference, end, mut source_basis) = fixture();
        source_basis.origin = [3., 4., 25.];
        let target = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        }
        .basis()
        .unwrap();
        let resolve = |sketch: &Sketch| {
            project_sketch_line(
                reference,
                FeatureId(1),
                sketch,
                &source_basis,
                FeatureId(2),
                &target,
            )
            .unwrap()
        };
        let initial = resolve(&sketch);
        assert_eq!(initial.start, Vec2::new(4., 6.));
        assert_eq!(initial.end, Vec2::new(8., 11.));
        *sketch.entity_mut(end).unwrap() = Entity::point(9., 10.);
        let before = serde_json::to_value(sketch.snapshot()).unwrap();
        let updated = resolve(&sketch);
        assert_eq!(updated.source, initial.source);
        assert_eq!(updated.end, Vec2::new(12., 14.));
        assert_eq!(serde_json::to_value(sketch.snapshot()).unwrap(), before);
        assert_eq!(initial.end, Vec2::new(8., 11.));
    }

    #[test]
    fn missing_and_wrong_owner_never_fall_back_to_another_curve() {
        let (sketch, reference, _, basis) = fixture();
        let resolve =
            |r, owner, target| project_sketch_line(r, owner, &sketch, &basis, target, &basis);
        assert_eq!(
            resolve(reference, FeatureId(3), FeatureId(2)),
            Err(ProjectionError::WrongSourceOwner)
        );
        assert_eq!(
            resolve(reference, FeatureId(1), FeatureId(1)),
            Err(ProjectionError::SelfReference)
        );
        assert_eq!(
            resolve(
                SketchCurveRef {
                    entity: EntityId(999),
                    ..reference
                },
                FeatureId(1),
                FeatureId(2)
            ),
            Err(ProjectionError::MissingSource)
        );
        assert_eq!(
            resolve(
                SketchCurveRef {
                    entity: EntityId(1),
                    ..reference
                },
                FeatureId(1),
                FeatureId(2)
            ),
            Err(ProjectionError::UnsupportedCurve)
        );
    }

    #[test]
    fn invalid_frames_endpoints_and_edge_on_projection_are_rejected() {
        let (mut sketch, reference, end, basis) = fixture();
        let mut invalid = basis;
        invalid.u = [2., 0., 0.];
        assert_eq!(
            project_sketch_line(
                reference,
                FeatureId(1),
                &sketch,
                &basis,
                FeatureId(2),
                &invalid
            ),
            Err(ProjectionError::InvalidBasis)
        );
        *sketch.entity_mut(end).unwrap() = Entity::point(f64::NAN, 7.);
        assert_eq!(
            project_sketch_line(
                reference,
                FeatureId(1),
                &sketch,
                &basis,
                FeatureId(2),
                &basis
            ),
            Err(ProjectionError::NonFiniteCoordinates)
        );
        *sketch.entity_mut(end).unwrap() = Entity::circle(0., 0., 1.);
        assert_eq!(
            project_sketch_line(
                reference,
                FeatureId(1),
                &sketch,
                &basis,
                FeatureId(2),
                &basis
            ),
            Err(ProjectionError::MissingEndpoint)
        );
        *sketch.entity_mut(end).unwrap() = Entity::point(5., 2.);
        let target = PlaneRef::OriginPlane {
            plane: OriginPlane::Yz,
        }
        .basis()
        .unwrap();
        assert_eq!(
            project_sketch_line(
                reference,
                FeatureId(1),
                &sketch,
                &basis,
                FeatureId(2),
                &target
            ),
            Err(ProjectionError::CollapsedLine)
        );
    }
}
