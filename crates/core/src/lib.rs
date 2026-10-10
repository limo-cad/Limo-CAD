//! limo-cad-core — Limo CAD document model.
//!
//! Owns the in-memory representation of a CAD document: its unit settings,
//! the browser tree shown in the UI, and the parametric feature tree. The
//! native and browser hosts exchange snapshots of this model via
//! [`DocumentDto`].

mod appearance;
mod browser;
mod document;
mod dto;
pub mod edge_chain;
mod feature;
mod ids;
mod material;
mod plane;
mod print_bed;
mod print_handoff;
mod print_heights;
mod print_intent;
mod print_zones;
mod units;

pub use appearance::{
    BodyAppearance, Rgba8, DEFAULT_BODY_COLOR, DEFAULT_BRAND, DEFAULT_FILAMENT_DIAMETER_MM,
    DEFAULT_FILAMENT_TYPE, DEFAULT_MATERIAL_NAME,
};
pub use browser::{BrowserNode, BrowserNodeKind, NodeId};
pub use document::Document;
pub use dto::DocumentDto;
pub use feature::{Feature, FeatureId, FeatureKind, FeatureStatus, FeatureTree};
pub use ids::{BodyId, EdgeId, FaceId};
pub use material::{
    MaterialDetails, MaterialPrintProfile, MaterialProperty, MaterialSource, MaterialValue,
};
pub use plane::{OriginPlane, PlaneBasis, PlaneError, PlaneRef};
pub use print_bed::{
    embedded_printer_catalog, PrintBedDto, PrintNozzleMode, PrintProfileSource, PrinterCatalogDto,
    PrinterExtruderDto, PrinterProfileDto,
};
pub use print_handoff::*;
pub use print_heights::*;
pub use print_intent::*;
pub use print_zones::*;
pub use units::{DimensionStyle, DocumentSettings, UnitSystem};
