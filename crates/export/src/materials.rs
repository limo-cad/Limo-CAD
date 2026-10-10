//! Material catalog presets for manufacturing appearance + slicer metadata.
//!
//! Source of truth: [`../presets/catalog.json`](../presets/catalog.json).
//! Engineering properties and slicer profiles share this same catalog and API.

use std::sync::OnceLock;

use limo_cad_core::{BodyAppearance, BodyId, Rgba8};
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
    #[serde(default)]
    material: Option<limo_cad_core::MaterialDetails>,
}

/// One selectable material, with optional engineering and printing properties.
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
    #[serde(default)]
    pub material: Option<limo_cad_core::MaterialDetails>,
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
            material: self.material.clone(),
        }
    }
}

/// Embedded unified catalog covering plastics, metals, and FDM ecosystems.
pub fn material_catalog() -> &'static [MaterialPreset] {
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
                    material: entry.material,
                })
                .collect()
        })
        .as_slice()
}

pub fn find_preset(id: &str) -> Option<&'static MaterialPreset> {
    material_catalog().iter().find(|preset| preset.id == id)
}

/// Resolve the catalog shorthand before dispatching the shared appearance
/// mutation. Desktop inboxes and headless tools must send the same full value
/// to the owning engine; deserializing a shorthand directly invents defaults.
pub fn resolve_body_appearance(arguments: &serde_json::Value) -> Result<BodyAppearance, String> {
    if arguments.get("material").is_some_and(|v| !v.is_null()) {
        let appearance: BodyAppearance =
            serde_json::from_value(arguments.clone()).map_err(|e| e.to_string())?;
        appearance.material.as_ref().unwrap().validate()?;
        return Ok(appearance);
    }
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
    for preset in material_catalog() {
        let brand = preset.brand.as_str();
        if !out.contains(&brand) {
            out.push(brand);
        }
    }
    out
}

pub fn presets_for_brand(brand: &str) -> Vec<&'static MaterialPreset> {
    material_catalog()
        .iter()
        .filter(|preset| preset.brand.eq_ignore_ascii_case(brand))
        .collect()
}

/// JSON snapshot for MCP / UI when a live engine call is preferred.
pub fn catalog_json() -> String {
    serde_json::to_string_pretty(material_catalog()).expect("catalog serializes")
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
    fn unified_catalog_preserves_units_context_and_provenance() {
        let mut ids = std::collections::BTreeSet::new();
        for preset in material_catalog() {
            assert!(ids.insert(&preset.id));
            let material = preset.material.as_ref().unwrap();
            material.validate().unwrap();
            assert_eq!(material.catalog_id, preset.id);
        }
        let metal = find_preset("material.aluminum-6061-t6")
            .unwrap()
            .material
            .as_ref()
            .unwrap();
        assert_eq!(metal.kind, "metal");
        assert!(metal.print_profiles.is_empty());
        assert!(metal.properties.iter().any(|p| p.name == "YoungsModulus"
            && p.unit == "Pa"
            && p.value == limo_cad_core::MaterialValue::Number(68_900_000_000.)));
        let plastic = find_preset("generic.pla.gray")
            .unwrap()
            .material
            .as_ref()
            .unwrap();
        assert!(plastic
            .properties
            .iter()
            .any(|p| p.context.starts_with("Engineering reference:")));
        assert!(plastic
            .properties
            .iter()
            .any(|p| p.context.starts_with("Print profile:")));
        assert!(plastic
            .sources
            .iter()
            .any(|s| s.repository == "FreeCAD/FreeCAD"));
        assert!(plastic
            .sources
            .iter()
            .any(|s| s.repository == "OrcaSlicer/OrcaSlicer"));
        assert!(plastic
            .warnings
            .iter()
            .any(|w| w.contains("not measured strength")));
        let branded = find_preset("bambu.pla.basic.red")
            .unwrap()
            .material
            .as_ref()
            .unwrap();
        assert!(
            !branded
                .properties
                .iter()
                .any(|p| p.context.starts_with("Engineering reference:")),
            "Generic plastic strength must not be assigned to a branded formulation"
        );
    }

    #[test]
    fn full_snapshot_survives_missing_preset_and_shorthand_resolves_catalog() {
        let mut appearance = find_preset("generic.pla.gray")
            .unwrap()
            .to_appearance(BodyId(7));
        appearance.preset_id = Some("removed-in-a-future-catalog".into());
        let serialized = serde_json::to_value(&appearance).unwrap();
        assert_eq!(resolve_body_appearance(&serialized).unwrap(), appearance);
        assert_eq!(
            resolve_body_appearance(
                &serde_json::json!({"body_id":7,"preset_id":"material.aluminum-6061-t6"})
            )
            .unwrap(),
            find_preset("material.aluminum-6061-t6")
                .unwrap()
                .to_appearance(BodyId(7))
        );
        let mut invalid = serialized;
        invalid["material"]["properties"][0]["source_id"] = serde_json::json!("missing");
        assert!(resolve_body_appearance(&invalid).is_err());
    }
}
