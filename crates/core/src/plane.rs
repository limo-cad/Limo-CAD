use std::fmt;

use serde::{Deserialize, Serialize};

use crate::FaceId;

/// One of the three origin datum planes of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginPlane {
    Xy,
    Xz,
    Yz,
}

/// Persistent reference to the plane a sketch lives on.
///
/// `PlanarFace` is resolved by the solid document because only it owns the
/// current face table. `DatumPlane` is resolved by the document manager from
/// construction-plane history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlaneRef {
    OriginPlane { plane: OriginPlane },
    PlanarFace { face_id: FaceId },
    DatumPlane { datum_id: FaceId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneError {
    Unsupported,
}

impl fmt::Display for PlaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlaneError::Unsupported => write!(f, "plane reference is not currently resolvable"),
        }
    }
}

impl std::error::Error for PlaneError {}

/// Failure to construct a finite orthogonal projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaneProjectionError {
    NonFiniteInput,
    ZeroNormal,
    OutOfRange,
}

impl fmt::Display for PlaneProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonFiniteInput => "plane origin, normal and point must be finite",
            Self::ZeroNormal => "projection plane normal must be nonzero",
            Self::OutOfRange => "plane projection exceeds finite coordinate range",
        })
    }
}

impl std::error::Error for PlaneProjectionError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneProjection {
    pub point: [f64; 3],
    /// Distance in model units, positive on the side pointed to by the normal.
    pub signed_distance: f64,
}

/// Concrete placement of a sketch or planar B-rep face in Z-up world space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlaneBasis {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub normal: [f64; 3],
}

impl PlaneBasis {
    /// Project onto the plane defined by `origin` and `normal`.
    ///
    /// The normal need not be unit length; its magnitude does not affect the
    /// result. The in-plane `u`/`v` axes are not used. This does not validate a
    /// sketch coordinate frame or resolve a persistent plane reference.
    pub fn project_point(&self, point: [f64; 3]) -> Result<PlaneProjection, PlaneProjectionError> {
        if self
            .origin
            .iter()
            .chain(&self.normal)
            .chain(&point)
            .any(|x| !x.is_finite())
        {
            return Err(PlaneProjectionError::NonFiniteInput);
        }
        // Scale before squaring so finite very large/small normals remain usable.
        let scale = self.normal.iter().map(|x| x.abs()).fold(0.0, f64::max);
        if scale == 0.0 {
            return Err(PlaneProjectionError::ZeroNormal);
        }
        let scaled = self.normal.map(|x| x / scale);
        let length = scaled[0].hypot(scaled[1]).hypot(scaled[2]);
        let normal = scaled.map(|x| x / length);
        let delta: [f64; 3] = std::array::from_fn(|i| point[i] - self.origin[i]);
        let distance = delta[0] * normal[0] + delta[1] * normal[1] + delta[2] * normal[2];
        let projected = std::array::from_fn(|i| (-distance).mul_add(normal[i], point[i]));
        if !distance.is_finite() || projected.iter().any(|x| !x.is_finite()) {
            return Err(PlaneProjectionError::OutOfRange);
        }
        Ok(PlaneProjection {
            point: projected,
            signed_distance: distance,
        })
    }

    pub fn to_3d(&self, p: [f64; 2]) -> [f64; 3] {
        [
            self.origin[0] + self.u[0] * p[0] + self.v[0] * p[1],
            self.origin[1] + self.u[1] * p[0] + self.v[1] * p[1],
            self.origin[2] + self.u[2] * p[0] + self.v[2] * p[1],
        ]
    }

    pub fn to_2d(&self, p: [f64; 3]) -> [f64; 2] {
        let d = [
            p[0] - self.origin[0],
            p[1] - self.origin[1],
            p[2] - self.origin[2],
        ];
        [
            d[0] * self.u[0] + d[1] * self.u[1] + d[2] * self.u[2],
            d[0] * self.v[0] + d[1] * self.v[1] + d[2] * self.v[2],
        ]
    }
}

impl PlaneRef {
    pub const ORIGIN_PLANES: [PlaneRef; 3] = [
        PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        },
        PlaneRef::OriginPlane {
            plane: OriginPlane::Xz,
        },
        PlaneRef::OriginPlane {
            plane: OriginPlane::Yz,
        },
    ];

    /// Resolve an origin plane. Body-face resolution belongs to the solid
    /// document so missing topology can be reported as a broken reference.
    pub fn origin_basis(&self) -> Result<PlaneBasis, PlaneError> {
        let PlaneRef::OriginPlane { plane } = *self else {
            return Err(PlaneError::Unsupported);
        };
        let (u, v, normal) = match plane {
            OriginPlane::Xy => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            OriginPlane::Xz => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
            OriginPlane::Yz => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        };
        Ok(PlaneBasis {
            origin: [0.0; 3],
            u,
            v,
            normal,
        })
    }

    /// Backward-compatible name used by the M1 sketch code.
    pub fn basis(&self) -> Result<PlaneBasis, PlaneError> {
        self.origin_basis()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    #[test]
    fn origin_bases_are_orthonormal_and_right_handed() {
        for plane in PlaneRef::ORIGIN_PLANES {
            let b = plane.origin_basis().unwrap();
            assert_eq!(dot(b.u, b.u), 1.0);
            assert_eq!(dot(b.v, b.v), 1.0);
            assert_eq!(dot(b.u, b.v), 0.0);
            assert_eq!(cross(b.u, b.v), b.normal);
        }
    }

    #[test]
    fn mapping_roundtrips() {
        let b = PlaneRef::OriginPlane {
            plane: OriginPlane::Xz,
        }
        .origin_basis()
        .unwrap();
        let p = [12.5, -3.0];
        let w = b.to_3d(p);
        assert_eq!(w, [12.5, 0.0, -3.0]);
        assert_eq!(b.to_2d(w), p);
    }

    fn near(actual: [f64; 3], expected: [f64; 3]) {
        for (a, e) in actual.into_iter().zip(expected) {
            assert!((a - e).abs() < 1e-12, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn projection_matches_translated_and_oblique_plane_oracles() {
        let mut basis = PlaneRef::ORIGIN_PLANES[0].origin_basis().unwrap();
        basis.origin = [0.0, 0.0, 1.0];
        let result = basis.project_point([3.0, 4.0, 5.0]).unwrap();
        near(result.point, [3.0, 4.0, 1.0]);
        assert_eq!(result.signed_distance, 4.0);
        // x + y + z = 3, independent of the in-plane sketch axes.
        basis.origin = [1.0; 3];
        basis.normal = [1.0; 3];
        let result = basis.project_point([4.0, 4.0, 4.0]).unwrap();
        near(result.point, [1.0; 3]);
        assert!((result.signed_distance - 27.0_f64.sqrt()).abs() < 1e-12);
        near(
            basis.project_point(result.point).unwrap().point,
            result.point,
        );
    }

    #[test]
    fn projection_is_normal_scale_invariant_and_orientation_sensitive() {
        let mut basis = PlaneRef::ORIGIN_PLANES[0].origin_basis().unwrap();
        for scale in [1e-300, 1.0, 1e300] {
            basis.normal = [0.0, 3.0 * scale, 4.0 * scale];
            let positive = basis.project_point([2.0, 3.0, 4.0]).unwrap();
            near(positive.point, [2.0, 0.0, 0.0]);
            assert!((positive.signed_distance - 5.0).abs() < 1e-12);
            basis.normal = basis.normal.map(|x| -x);
            let negative = basis.project_point([2.0, 3.0, 4.0]).unwrap();
            near(positive.point, negative.point);
            assert_eq!(positive.signed_distance, -negative.signed_distance);
        }
    }

    #[test]
    fn projection_rejects_invalid_and_unrepresentable_geometry() {
        let mut basis = PlaneRef::ORIGIN_PLANES[0].origin_basis().unwrap();
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                basis.project_point([invalid, 0.0, 0.0]),
                Err(PlaneProjectionError::NonFiniteInput)
            );
            basis.normal = [0.0, 0.0, invalid];
            assert_eq!(
                basis.project_point([0.0; 3]),
                Err(PlaneProjectionError::NonFiniteInput)
            );
            basis.normal = [0.0, 0.0, 1.0];
            basis.origin = [invalid, 0.0, 0.0];
            assert_eq!(
                basis.project_point([0.0; 3]),
                Err(PlaneProjectionError::NonFiniteInput)
            );
            basis.origin = [0.0; 3];
        }
        basis.normal = [0.0; 3];
        assert_eq!(
            basis.project_point([0.0; 3]),
            Err(PlaneProjectionError::ZeroNormal)
        );
        basis.normal = [0.0, 0.0, 1.0];
        basis.origin = [0.0, 0.0, -f64::MAX];
        assert_eq!(
            basis.project_point([0.0, 0.0, f64::MAX]),
            Err(PlaneProjectionError::OutOfRange)
        );
    }
}
