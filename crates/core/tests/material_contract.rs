use limo_cad_core::{
    physical_value_si, BodyAppearance, MaterialDetails, MaterialProperty, MaterialSource,
    MaterialValue, PhysicalDimension as D,
};

fn material(kind: &str, unit: &str, value: MaterialValue) -> MaterialDetails {
    MaterialDetails {
        kind: kind.into(),
        catalog_id: "test.synthetic-material".into(),
        properties: vec![MaterialProperty {
            name: "Density".into(),
            value,
            unit: unit.into(),
            context: "Synthetic regression fixture, not engineering data".into(),
            source_id: "fixture".into(),
        }],
        sources: vec![MaterialSource {
            id: "fixture".into(),
            repository: "test/fixtures".into(),
            revision: "1".repeat(40),
            path: "synthetic.json".into(),
            sha256: "2".repeat(64),
            license: "test".into(),
            author: "test".into(),
            reference: String::new(),
        }],
        ..MaterialDetails::default()
    }
}

#[test]
fn building_and_fluid_materials_roundtrip_the_existing_body_assignment() {
    for kind in [
        "plastic",
        "metal",
        "ceramic",
        "glass",
        "wood",
        "concrete",
        "masonry",
        "composite",
        "liquid",
        "gas",
        "other",
    ] {
        let material = material(kind, "g/cm^3", MaterialValue::Number(1.25));
        material.validate().unwrap();
        let appearance = BodyAppearance {
            material: Some(material.clone()),
            ..BodyAppearance::default_for(limo_cad_core::BodyId(7))
        };
        let saved = serde_json::to_string(&appearance).unwrap();
        let reopened: BodyAppearance = serde_json::from_str(&saved).unwrap();
        assert_eq!(reopened.material.as_ref(), Some(&material));
        assert_eq!(
            reopened.material.unwrap().properties[0]
                .value_si(D::Density)
                .unwrap(),
            1250.0
        );
    }
}

#[test]
fn engineering_conversions_match_independent_si_values() {
    for (value, unit, dimension, expected) in [
        (2.7, "g/cm^3", D::Density, 2700.0),
        (1.0, "kg/L", D::Density, 1000.0),
        (0.001, "g/mm^3", D::Density, 1000.0),
        (210.0, "GPa", D::Pressure, 210_000_000_000.0),
        (25.0, "N/mm^2", D::Pressure, 25_000_000.0),
        (1.0, "mPa", D::Pressure, 0.001),
        (1.0, "MPa", D::Pressure, 1_000_000.0),
        (1.0, "cP", D::DynamicViscosity, 0.001),
        (0.002, "W/(mm*K)", D::ThermalConductivity, 2.0),
        (4.18, "kJ/(kg*K)", D::SpecificHeatCapacity, 4180.0),
        (12.0, "um/(m*K)", D::ThermalExpansionCoefficient, 0.000_012),
        (68.0, "µm/m/K", D::ThermalExpansionCoefficient, 0.000_068),
        (58.0, "MS/m", D::ElectricalConductivity, 58_000_000.0),
        (30.0, "%", D::Dimensionless, 0.3),
    ] {
        let actual = physical_value_si(value, unit, dimension).unwrap();
        assert!(
            (actual - expected).abs() <= expected.abs().max(1e-12) * 1e-12,
            "{value} {unit}: {actual} != {expected}"
        );
    }
    for dimension in [
        D::Dimensionless,
        D::Density,
        D::Pressure,
        D::DynamicViscosity,
        D::ThermalConductivity,
        D::SpecificHeatCapacity,
        D::ThermalExpansionCoefficient,
        D::ElectricalConductivity,
    ] {
        assert_eq!(
            physical_value_si(2.0, dimension.si_unit(), dimension).unwrap(),
            2.0
        );
    }
}

#[test]
fn numerical_consumers_reject_unknown_or_wrong_dimensions_and_text() {
    for (unit, dimension) in [
        ("MPa", D::Density),
        ("kg/m^3", D::Pressure),
        ("", D::Pressure),
        ("mpa", D::Pressure),
        ("vendor_unit", D::Density),
        ("m^2/s", D::DynamicViscosity),
    ] {
        assert!(physical_value_si(1.0, unit, dimension).is_err(), "{unit}");
    }
    let text = material("wood", "kg/m^3", MaterialValue::Text("500".into()));
    text.validate().unwrap();
    assert!(text.properties[0].value_si(D::Density).is_err());
}

#[test]
fn unsupported_properties_remain_preserved_but_are_not_assumed_numeric_si() {
    let source = material("concrete", "vendor_unit", MaterialValue::Number(2.0));
    source.validate().unwrap();
    assert!(source.properties[0].value_si(D::Density).is_err());
    let reopened: MaterialDetails =
        serde_json::from_str(&serde_json::to_string(&source).unwrap()).unwrap();
    assert_eq!(reopened, source);
}

#[test]
fn conversion_rejects_nonfinite_overflow_and_underflow_without_mutation() {
    for (value, unit) in [
        (f64::NAN, "Pa"),
        (f64::INFINITY, "Pa"),
        (f64::NEG_INFINITY, "Pa"),
        (f64::MAX, "GPa"),
        (f64::from_bits(1), "mPa"),
    ] {
        assert!(physical_value_si(value, unit, D::Pressure).is_err());
    }
    let source = material("metal", "GPa", MaterialValue::Number(f64::MAX));
    let before = source.clone();
    assert!(source.properties[0].value_si(D::Pressure).is_err());
    assert_eq!(source, before);
}

#[test]
fn broader_families_do_not_weaken_identity_and_source_validation() {
    let mut source = material("liquid", "kg/m^3", MaterialValue::Number(1000.0));
    source.kind = "invalid".into();
    assert!(source.validate().is_err());
    source.kind = "liquid".into();
    source.properties[0].source_id = "missing".into();
    assert!(source.validate().is_err());
    source.properties[0].source_id = "fixture".into();
    source.properties[0].value = MaterialValue::Number(f64::INFINITY);
    assert!(source.validate().is_err());
}
