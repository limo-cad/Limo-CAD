//! Host-neutral solid modeling contract.
//!
//! Rust owns persistent feature definitions, rollback/recompute planning,
//! stable topology ids, reference validation, and serialized mesh DTOs.
//! Kernel adapters consume [`RecomputePlanDto`] and return [`KernelSceneDto`].

mod dto;
mod history;
mod profile;
mod stable;
mod thread;
mod topology;

pub use dto::*;
pub use history::{
    hole_reference_center, ordered_path, pattern_copy_count, plane_bases_coplanar,
    validate_external_thread, validate_hole, validate_rib_extent, SolidDocument, SolidError,
    SolidFeatureDefinitions,
};
pub use profile::{
    canonicalize_profile_curves, extract_bounded_faces, extract_closed_loops,
    extract_closed_loops_allow_open, BoundedFace, ProfileError, Segment2,
};
pub use stable::face_id as stable_face_id;
pub use thread::{
    iso_metric_grade6_envelope, iso_metric_thread_envelope, rounded_thread_diameters,
    IsoMetricThreadEnvelope, ThreadFit,
};
pub use topology::{edge_is_straight, tangent_chain_edges};
