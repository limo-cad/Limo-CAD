//! Per-body appearance for viewport tint and manufacturing export (3MF).
//!
//! Source of truth for part color/material lives here and in `.limo`
//! (`body_appearances`). Viewport engines consume it; they do not author it.
//! STEP export does not invent colors from this store.
//!
//! Additive fields (`filament_type`, `brand`, …) use `#[serde(default)]` so
//! older projects remain readable without a schema bump.

use serde::{Deserialize, Serialize};

use crate::ids::BodyId;

/// Default viewport body gray (~CSS `--cad-body` mid tone).
pub const DEFAULT_BODY_COLOR: Rgba8 = Rgba8 {
    r: 180,
    g: 180,
    b: 180,
    a: 255,
};

pub const DEFAULT_MATERIAL_NAME: &str = "Generic";
pub const DEFAULT_FILAMENT_TYPE: &str = "PLA";
pub const DEFAULT_BRAND: &str = "Generic";
pub const DEFAULT_FILAMENT_DIAMETER_MM: f64 = 1.75;

/// 8-bit RGBA color. Alpha is stored for UI preview; 3MF v1 writes opaque RGB
/// when `a < 255` because many slicers ignore alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Flatten to opaque RGB for formats that ignore alpha.
    pub const fn opaque_rgb(self) -> Self {
        Self {
            r: self.r,
            g: self.g,
            b: self.b,
            a: 255,
        }
    }

    pub fn to_hex_rgb(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl Default for Rgba8 {
    fn default() -> Self {
        DEFAULT_BODY_COLOR
    }
}

/// Per-body color and print material for display and 3MF / slicer metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyAppearance {
    pub body_id: BodyId,
    #[serde(default)]
    pub color: Rgba8,
    /// Human-facing material / filament label (e.g. "Bambu PLA Basic Red").
    #[serde(default = "default_material_name")]
    pub material_name: String,
    /// Filament chemistry family: PLA, PETG, ABS, ASA, TPU, PA, PC, …
    /// An explicit empty value denotes no filament family; omitted legacy fields use PLA.
    #[serde(default = "default_filament_type")]
    pub filament_type: String,
    /// Vendor / ecosystem: Generic, Bambu Lab, Prusa, Polymaker, …
    /// An explicit empty value denotes an unspecified vendor.
    #[serde(default = "default_brand")]
    pub brand: String,
    /// Official or marketing color name (e.g. "Jade White").
    #[serde(default)]
    pub color_name: String,
    /// Optional vendor profile / SKU id (Bambu `filament_ids`, Prusa preset id).
    #[serde(default)]
    pub filament_id: Option<String>,
    /// Catalog preset key from BodyAppearancePanel or MCP. The distributed 3MF
    /// names the material from `filament_type` and `color_name`, not from this
    /// printer preset string.
    #[serde(default)]
    pub preset_id: Option<String>,
    /// Filament density g/cm³ when known (feeds slicer metadata).
    #[serde(default)]
    pub density_g_cm3: Option<f64>,
    /// Filament diameter in millimetres (default 1.75).
    #[serde(default = "default_filament_diameter")]
    pub diameter_mm: f64,
    /// Frozen merged engineering and printing properties; absent in old files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<crate::MaterialDetails>,
}

fn default_material_name() -> String {
    DEFAULT_MATERIAL_NAME.to_string()
}

fn default_filament_type() -> String {
    DEFAULT_FILAMENT_TYPE.to_string()
}

fn default_brand() -> String {
    DEFAULT_BRAND.to_string()
}

fn default_filament_diameter() -> f64 {
    DEFAULT_FILAMENT_DIAMETER_MM
}

impl BodyAppearance {
    pub fn default_for(body_id: BodyId) -> Self {
        Self {
            body_id,
            color: DEFAULT_BODY_COLOR,
            material_name: DEFAULT_MATERIAL_NAME.to_string(),
            filament_type: DEFAULT_FILAMENT_TYPE.to_string(),
            brand: DEFAULT_BRAND.to_string(),
            color_name: String::new(),
            filament_id: None,
            preset_id: None,
            density_g_cm3: None,
            material: None,
            diameter_mm: DEFAULT_FILAMENT_DIAMETER_MM,
        }
    }

    /// Prefer color_name, else material_name, for human-facing labels (e.g. Cura
    /// hint JSON). Consortium 3MF basematerials use `filament_type` + `color_name`
    /// via the export crate's portable material name, not this helper.
    pub fn display_label(&self) -> &str {
        if !self.color_name.trim().is_empty() {
            self.color_name.as_str()
        } else if !self.material_name.trim().is_empty() {
            self.material_name.as_str()
        } else {
            DEFAULT_MATERIAL_NAME
        }
    }
}

impl PartialEq for BodyAppearance {
    fn eq(&self, other: &Self) -> bool {
        self.body_id == other.body_id
            && self.color == other.color
            && self.material_name == other.material_name
            && self.filament_type == other.filament_type
            && self.brand == other.brand
            && self.color_name == other.color_name
            && self.filament_id == other.filament_id
            && self.preset_id == other.preset_id
            && float_eq(self.density_g_cm3, other.density_g_cm3)
            && self.material == other.material
            && (self.diameter_mm - other.diameter_mm).abs() < 1e-9
    }
}

fn float_eq(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => false,
    }
}

impl Eq for BodyAppearance {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_roundtrips_json() {
        let appearance = BodyAppearance {
            body_id: BodyId(3),
            color: Rgba8::opaque(255, 64, 32),
            material_name: "PLA Red".to_string(),
            filament_type: "PLA".to_string(),
            brand: "Bambu Lab".to_string(),
            color_name: "Red".to_string(),
            filament_id: Some("GFA00".into()),
            preset_id: Some("bambu.pla.basic.red".into()),
            density_g_cm3: Some(1.24),
            material: None,
            diameter_mm: 1.75,
        };
        let json = serde_json::to_string(&appearance).unwrap();
        let back: BodyAppearance = serde_json::from_str(&json).unwrap();
        assert_eq!(back, appearance);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let back: BodyAppearance = serde_json::from_str(r#"{"body_id":1}"#).unwrap();
        assert_eq!(back.body_id, BodyId(1));
        assert_eq!(back.color, DEFAULT_BODY_COLOR);
        assert_eq!(back.material_name, DEFAULT_MATERIAL_NAME);
        assert_eq!(back.filament_type, DEFAULT_FILAMENT_TYPE);
        assert_eq!(back.brand, DEFAULT_BRAND);
        assert!((back.diameter_mm - DEFAULT_FILAMENT_DIAMETER_MM).abs() < 1e-9);
    }
}
