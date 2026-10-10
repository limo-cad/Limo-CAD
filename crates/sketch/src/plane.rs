//! Compatibility re-exports. Plane references and stable face ids moved to
//! `limo-cad-core` in M2 so sketches and solids share one contract.

pub use limo_cad_core::{FaceId, OriginPlane, PlaneBasis, PlaneError, PlaneRef};
