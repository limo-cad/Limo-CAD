//! One resolved material record, persisted with the body's appearance.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MaterialValue {
    Number(f64),
    Text(String),
}
impl std::fmt::Display for MaterialValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(v) => write!(f, "{v}"),
            Self::Text(v) => f.write_str(v),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialProperty {
    pub name: String,
    pub value: MaterialValue,
    pub unit: String,
    /// Engineering reference, filament, or printer-specific context.
    pub context: String,
    pub source_id: String,
}

impl MaterialProperty {
    /// Read one explicitly selected property in SI without modifying its saved
    /// value, unit, context or provenance. The consumer must select the correct
    /// property/context and enforce its physical model's applicability bounds.
    pub fn value_si(&self, expected: crate::PhysicalDimension) -> Result<f64, String> {
        let MaterialValue::Number(value) = self.value else {
            return Err(format!("Material property '{}' is not numeric", self.name));
        };
        crate::physical_value_si(value, &self.unit, expected)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialSource {
    pub id: String,
    pub repository: String,
    pub revision: String,
    pub path: String,
    pub sha256: String,
    pub license: String,
    pub author: String,
    #[serde(default)]
    pub reference: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialPrintProfile {
    pub name: String,
    pub source_id: String,
    pub compatible_printers: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDetails {
    /// Material family, not solver applicability or phase inference. Existing
    /// plastic/metal keys remain unchanged; fluid and building families share
    /// this same persisted record and provenance model.
    pub kind: String,
    pub catalog_id: String,
    #[serde(default)]
    pub properties: Vec<MaterialProperty>,
    #[serde(default)]
    pub sources: Vec<MaterialSource>,
    #[serde(default)]
    pub print_profiles: Vec<MaterialPrintProfile>,
    #[serde(default)]
    pub warnings: Vec<String>,
}
impl MaterialDetails {
    /// Select a unique, validated property without guessing between engineering
    /// references and print profiles. Optional filters are exact matches, not
    /// prefixes. Even numerically equal records remain ambiguous when their
    /// provenance differs. Callers can display candidates before selecting.
    pub fn select_property(
        &self,
        name: &str,
        context: Option<&str>,
        source_id: Option<&str>,
    ) -> Result<&MaterialProperty, String> {
        self.validate()?;
        let mut candidates = self.properties.iter().filter(|property| {
            property.name == name
                && context.is_none_or(|value| property.context == value)
                && source_id.is_none_or(|value| property.source_id == value)
        });
        let selected = candidates.next().ok_or_else(|| {
            format!("No material property '{name}' matches the selected context/source")
        })?;
        if candidates.next().is_some() {
            return Err(format!(
                "Material property '{name}' is ambiguous; select its exact context and source"
            ));
        }
        Ok(selected)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(
            self.kind.as_str(),
            "plastic"
                | "metal"
                | "ceramic"
                | "glass"
                | "wood"
                | "concrete"
                | "masonry"
                | "composite"
                | "liquid"
                | "gas"
                | "other"
        ) || self.catalog_id.is_empty()
            || self.catalog_id.len() > 256
            || self.properties.len() > 2048
            || self.sources.len() > 64
            || self.print_profiles.len() > 64
            || self.warnings.len() > 64
        {
            return Err("Invalid material identity or excessive material data".into());
        }
        let mut ids = BTreeSet::new();
        for s in &self.sources {
            if !ids.insert(s.id.as_str())
                || s.id.is_empty()
                || s.id.len() > 4096
                || s.revision.len() != 40
                || !s.revision.bytes().all(|c| c.is_ascii_hexdigit())
                || s.sha256.len() != 64
                || !s.sha256.bytes().all(|c| c.is_ascii_hexdigit())
                || [&s.repository, &s.path, &s.license, &s.author, &s.reference]
                    .iter()
                    .any(|v| v.len() > 4096)
            {
                return Err("Invalid material source provenance".into());
            }
        }
        for p in &self.properties {
            if p.name.is_empty()
                || p.name.len() > 256
                || p.context.len() > 512
                || p.context.is_empty()
                || p.unit.len() > 128
                || !ids.contains(p.source_id.as_str())
                || match &p.value {
                    MaterialValue::Number(v) => !v.is_finite(),
                    MaterialValue::Text(v) => v.len() > 4096,
                }
            {
                return Err("Invalid material property or source reference".into());
            }
        }
        for p in &self.print_profiles {
            if !ids.contains(p.source_id.as_str())
                || p.name.len() > 512
                || p.name.is_empty()
                || p.compatible_printers.len() > 1024
                || p.compatible_printers.iter().any(|v| v.len() > 512)
            {
                return Err("Invalid material print profile".into());
            }
        }
        if self.warnings.iter().any(|v| v.len() > 4096) {
            return Err("Invalid material warning".into());
        }
        Ok(())
    }
}
