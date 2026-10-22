use serde::{Deserialize, Serialize};

/// Scalar engineering dimensions supported by the material boundary.
/// This is not a general expression parser or a constitutive material model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalDimension {
    Dimensionless,
    Density,
    Pressure,
    DynamicViscosity,
    ThermalConductivity,
    SpecificHeatCapacity,
    ThermalExpansionCoefficient,
    ElectricalConductivity,
}

impl PhysicalDimension {
    pub const fn si_unit(self) -> &'static str {
        match self {
            Self::Dimensionless => "1",
            Self::Density => "kg/m^3",
            Self::Pressure => "Pa",
            Self::DynamicViscosity => "Pa*s",
            Self::ThermalConductivity => "W/(m*K)",
            Self::SpecificHeatCapacity => "J/(kg*K)",
            Self::ThermalExpansionCoefficient => "1/K",
            Self::ElectricalConductivity => "S/m",
        }
    }
}

/// Convert an explicitly supported scalar unit to SI. Unit spelling is case
/// sensitive: mPa and MPa must never be conflated. Unknown units and dimensional
/// mismatches fail instead of silently assuming SI. Original source values are
/// not rewritten by this conversion.
pub fn physical_value_si(
    value: f64,
    unit: &str,
    expected: PhysicalDimension,
) -> Result<f64, String> {
    use PhysicalDimension::*;
    let (dimension, scale) = match unit.trim() {
        "" | "1" => (Dimensionless, 1.0),
        "%" => (Dimensionless, 0.01),
        "kg/m^3" | "kg/m³" => (Density, 1.0),
        "g/cm^3" | "g/cm³" | "kg/L" => (Density, 1000.0),
        "g/mm^3" | "g/mm³" => (Density, 1_000_000.0),
        "Pa" | "N/m^2" | "N/m²" => (Pressure, 1.0),
        "mPa" => (Pressure, 0.001),
        "kPa" => (Pressure, 1000.0),
        "MPa" | "N/mm^2" | "N/mm²" => (Pressure, 1_000_000.0),
        "GPa" => (Pressure, 1_000_000_000.0),
        "Pa*s" | "Pa s" | "kg/(m*s)" => (DynamicViscosity, 1.0),
        "mPa*s" | "mPa s" | "cP" => (DynamicViscosity, 0.001),
        "W/(m*K)" | "W/m/K" => (ThermalConductivity, 1.0),
        "W/(mm*K)" => (ThermalConductivity, 1000.0),
        "J/(kg*K)" | "J/kg/K" => (SpecificHeatCapacity, 1.0),
        "kJ/(kg*K)" | "J/(g*K)" => (SpecificHeatCapacity, 1000.0),
        "1/K" | "K^-1" | "m/m/K" => (ThermalExpansionCoefficient, 1.0),
        "um/(m*K)" | "µm/(m*K)" | "µm/m/K" => (ThermalExpansionCoefficient, 0.000_001),
        "S/m" => (ElectricalConductivity, 1.0),
        "S/mm" | "kS/m" => (ElectricalConductivity, 1000.0),
        "MS/m" => (ElectricalConductivity, 1_000_000.0),
        _ => return Err(format!("Unsupported physical unit '{unit}'")),
    };
    if dimension != expected {
        return Err(format!(
            "Physical unit '{unit}' has dimension {dimension:?}, expected {expected:?}"
        ));
    }
    let converted = value * scale;
    if !value.is_finite() || !converted.is_finite() || (value != 0.0 && converted == 0.0) {
        return Err("Physical value conversion is not finite or representable".into());
    }
    Ok(converted)
}

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
