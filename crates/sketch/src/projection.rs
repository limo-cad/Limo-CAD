//! Development foundation for explicit associative Projection (#382).
//!
//! Resolve a sketch line by its owning feature and entity, then project its
//! current endpoints. This is a read-only preview primitive, not a persisted
//! reference or a public host command. Manager dependency validation, native
//! edge lineage, persistence and UI integration remain separate work.
use crate::{Entity, EntityId, Sketch, Vec2};
use limo_cad_core::{FeatureId, PlaneBasis};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SketchCurveRef {
    pub feature: FeatureId,
    pub entity: EntityId,
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
