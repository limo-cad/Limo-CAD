//! Slicer / ecosystem export targets for 3MF packages.
//!
//! The file to distribute is a standard 3MF: mesh, millimetres, a portable
//! material name, and a display color. `standard`, `bambu_studio`, and
//! `orca_slicer` all write that package. A Bambu or Orca project profile
//! belongs in a file saved from that slicer, not in the CAD export.
//! PrusaSlicer still gets `Slic3r_PE` hints because it ignores basematerials.
//! Cura gets consortium basematerials plus `Metadata/cura_materials.json`.

use serde::{Deserialize, Serialize};

/// Which slicer ecosystem to optimize 3MF metadata for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SlicerTarget {
    /// Consortium 3MF: mesh, millimetres, portable material name, display color.
    /// This is the file to distribute. Slicer projects are saved from the slicer.
    #[default]
    Standard,
    /// Same portable package as `Standard`. Kept so existing callers still resolve.
    BambuStudio,
    /// Same portable package as `Standard`. Kept so existing callers still resolve.
    OrcaSlicer,
    /// PrusaSlicer / SuperSlicer Slic3r_PE model config hints.
    PrusaSlicer,
    /// UltiMaker Cura — basematerials + cura_materials.json hints.
    Cura,
}
impl SlicerTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::BambuStudio => "bambu_studio",
            Self::OrcaSlicer => "orca_slicer",
            Self::PrusaSlicer => "prusa_slicer",
            Self::Cura => "cura",
        }
    }

    pub fn application_metadata(self) -> &'static str {
        match self {
            Self::Standard | Self::BambuStudio | Self::OrcaSlicer => "Limo CAD",
            Self::PrusaSlicer => "Limo CAD (PrusaSlicer-compatible)",
            Self::Cura => "Limo CAD (Cura-compatible)",
        }
    }

    pub fn all() -> &'static [SlicerTarget] {
        &[
            Self::Standard,
            Self::BambuStudio,
            Self::OrcaSlicer,
            Self::PrusaSlicer,
            Self::Cura,
        ]
    }
}
