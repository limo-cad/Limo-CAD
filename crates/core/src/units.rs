use serde::{Deserialize, Serialize};

/// Length unit system of a document.
///
/// Defaults to millimeters. Serializes as a plain snake_case string (`"mm"`)
/// for IPC with the UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitSystem {
    #[default]
    Mm,
    Cm,
    In,
}

/// Dimension annotation style: text aligned to the measured geometry, or
/// upright ISO 129 presentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionStyle {
    #[default]
    Aligned,
    Iso,
}

/// Document-level settings. Lives under "Document Settings" in the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DocumentSettings {
    pub units: UnitSystem,
    pub dimension_style: DimensionStyle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_units_are_mm() {
        assert_eq!(UnitSystem::default(), UnitSystem::Mm);
        assert_eq!(DocumentSettings::default().units, UnitSystem::Mm);
        assert_eq!(
            DocumentSettings::default().dimension_style,
            DimensionStyle::Aligned
        );
    }

    #[test]
    fn units_serialize_as_snake_case() {
        assert_eq!(serde_json::to_string(&UnitSystem::Mm).unwrap(), "\"mm\"");
        assert_eq!(
            serde_json::to_string(&DimensionStyle::Iso).unwrap(),
            "\"iso\""
        );
        assert_eq!(
            serde_json::to_string(&DimensionStyle::Aligned).unwrap(),
            "\"aligned\""
        );
    }
}
