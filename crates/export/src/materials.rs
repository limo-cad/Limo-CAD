//! Material catalog presets for manufacturing appearance + slicer metadata.
//!
//! Source of truth: [`../presets/catalog.json`](../presets/catalog.json).
//! The TypeScript UI imports the same JSON via Vite so brands stay aligned.

use std::sync::OnceLock;

use nbcad_core::{BodyAppearance, BodyId, Rgba8};
use serde::{Deserialize, Serialize};

const CATALOG_JSON: &str = include_str!("../presets/catalog.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CatalogEntry {
    id: String,
    brand: String,
    filament_type: String,
    material_name: String,
    color_name: String,
    r: u8,
    g: u8,
    b: u8,
    filament_id: Option<String>,
    density_g_cm3: Option<f64>,
    diameter_mm: f64,
}

/// One selectable filament color / profile entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialPreset {
    pub id: String,
    pub brand: String,
    pub filament_type: String,
    pub material_name: String,
    pub color_name: String,
    pub color: Rgba8,
    pub filament_id: Option<String>,
    pub density_g_cm3: Option<f64>,
    pub diameter_mm: f64,
}

impl MaterialPreset {
    pub fn to_appearance(&self, body_id: BodyId) -> BodyAppearance {
        BodyAppearance {
            body_id,
            color: self.color,
            material_name: self.material_name.clone(),
            filament_type: self.filament_type.clone(),
            brand: self.brand.clone(),
            color_name: self.color_name.clone(),
            filament_id: self.filament_id.clone(),
            preset_id: Some(self.id.clone()),
            density_g_cm3: self.density_g_cm3,
            diameter_mm: self.diameter_mm,
        }
    }
}

fn catalog_entries() -> &'static [MaterialPreset] {
    static CATALOG: OnceLock<Vec<MaterialPreset>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let raw: Vec<CatalogEntry> =
                serde_json::from_str(CATALOG_JSON).expect("material catalog JSON must parse");
            raw.into_iter()
                .map(|entry| MaterialPreset {
                    id: entry.id,
                    brand: entry.brand,
                    filament_type: entry.filament_type,
                    material_name: entry.material_name,
                    color_name: entry.color_name,
                    color: Rgba8::opaque(entry.r, entry.g, entry.b),
                    filament_id: entry.filament_id,
                    density_g_cm3: entry.density_g_cm3,
                    diameter_mm: entry.diameter_mm,
                })
                .collect()
        })
        .as_slice()
}

/// Built-in catalog covering Generic + major FDM ecosystems.
pub fn material_catalog() -> &'static [MaterialPreset] {
    catalog_entries()
}

pub fn find_preset(id: &str) -> Option<&'static MaterialPreset> {
    catalog_entries().iter().find(|preset| preset.id == id)
}

/// Resolve the catalog shorthand before dispatching the shared appearance
/// mutation. Desktop inboxes and headless tools must send the same full value
/// to the owning engine; deserializing a shorthand directly invents defaults.
pub fn resolve_body_appearance(arguments: &serde_json::Value) -> Result<BodyAppearance, String> {
    if let Some(preset_id) = arguments
        .get("preset_id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
    {
        let body_id = arguments
            .get("body_id")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "set_body_appearance with preset_id requires body_id".to_string())?;
        let preset = find_preset(preset_id).ok_or_else(|| {
            format!("unknown material preset_id '{preset_id}' (call material_catalog)")
        })?;
        Ok(preset.to_appearance(BodyId(body_id)))
    } else {
        serde_json::from_value(arguments.clone()).map_err(|error| {
            format!("invalid body appearance (or pass body_id + preset_id): {error}")
        })
    }
}

pub fn brands() -> Vec<&'static str> {
    let mut out = Vec::new();
    for preset in catalog_entries() {
        let brand = preset.brand.as_str();
        if !out.contains(&brand) {
            out.push(brand);
        }
    }
    out
}

pub fn presets_for_brand(brand: &str) -> Vec<&'static MaterialPreset> {
    catalog_entries()
        .iter()
        .filter(|preset| preset.brand.eq_ignore_ascii_case(brand))
        .collect()
}

/// JSON snapshot for MCP / UI when a live engine call is preferred.
pub fn catalog_json() -> String {
    serde_json::to_string_pretty(catalog_entries()).expect("catalog serializes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_bambu_and_prusa() {
        assert!(brands().contains(&"Bambu Lab"));
        assert!(brands().contains(&"Prusa"));
        assert!(brands().contains(&"Sunlu"));
        assert!(brands().contains(&"eSun"));
        assert!(brands().contains(&"Anycubic"));
        assert!(find_preset("bambu.pla.basic.red").is_some());
        assert!(find_preset("bambu.paht.cf.black").is_some());
        assert!(find_preset("prusa.pla.msasaki_orange").is_some());
        assert!(material_catalog().len() >= 40);
    }

    #[test]
    fn preset_maps_to_appearance() {
        let preset = find_preset("bambu.pla.basic.red").unwrap();
        let appearance = preset.to_appearance(BodyId(7));
        assert_eq!(appearance.body_id, BodyId(7));
        assert_eq!(appearance.brand, "Bambu Lab");
        assert_eq!(appearance.filament_type, "PLA");
        assert_eq!(appearance.color.r, 200);
        assert_eq!(appearance.preset_id.as_deref(), Some("bambu.pla.basic.red"));
    }

    #[test]
    fn catalog_json_roundtrips_count() {
        let value: serde_json::Value = serde_json::from_str(&catalog_json()).unwrap();
        assert_eq!(value.as_array().unwrap().len(), material_catalog().len());
    }

    #[test]
    fn frontend_catalog_mirror_matches_source() {
        let frontend = include_str!("../../../src/materials/catalog.json");
        assert_eq!(
            CATALOG_JSON, frontend,
            "run the ignored regen_frontend_catalog_mirror test after editing the catalog"
        );
    }

    /// Copies `presets/catalog.json` into `src/materials/catalog.json` for the Vite UI.
    /// Run explicitly when the preset catalog changes: `cargo test -p nbcad-export regen_frontend_catalog_mirror -- --ignored --exact`
    #[test]
    #[ignore]
    fn regen_frontend_catalog_mirror() {
        use std::fs;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let source = manifest_dir.join("presets/catalog.json");
        let dest = manifest_dir.join("../../src/materials/catalog.json");
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).expect("create src/materials/");
        }
        let bytes = fs::read(&source).expect("read presets/catalog.json");
        fs::write(&dest, &bytes).expect("mirror catalog.json into src/materials/");
    }
}
