use limo_cad_core::{
    MaterialDetails, MaterialProperty, MaterialPropertyFilter as Filter, MaterialSource,
    MaterialValue,
};

fn material(e: f64, nu: f64, k: f64) -> MaterialDetails {
    MaterialDetails {
        kind: "other".into(),
        catalog_id: "test.constitutive".into(),
        properties: [
            ("YoungsModulus", e, "GPa"),
            ("PoissonRatio", nu, "1"),
            ("ThermalConductivity", k, "W/(m*K)"),
        ]
        .into_iter()
        .map(|(name, value, unit)| MaterialProperty {
            name: name.into(),
            value: MaterialValue::Number(value),
            unit: unit.into(),
            context: "Synthetic".into(),
            source_id: "fixture".into(),
        })
        .collect(),
        sources: vec![MaterialSource {
            id: "fixture".into(),
            repository: "test".into(),
            revision: "1".repeat(40),
            path: "synthetic".into(),
            sha256: "2".repeat(64),
            license: "test".into(),
            author: "test".into(),
            reference: String::new(),
        }],
        ..Default::default()
    }
}
fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-10 * expected.abs().max(1.),
        "{actual} != {expected}"
    );
}

#[test]
fn uniaxial_hydrostatic_and_shear_responses_have_independent_physical_oracles() {
    let m = material(210., 0.3, 2.);
    let law = m
        .isotropic_elasticity(Filter::default(), Filter::default())
        .unwrap();
    close(law.youngs_modulus_pa(), 210e9);
    close(law.poisson_ratio(), 0.3);
    close(law.bulk_modulus_pa(), 175e9);
    close(law.shear_modulus_pa(), 80_769_230_769.23077);
    // Free lateral contraction under a 0.1% axial extension.
    let stress = law
        .stress_pa([0.001, -0.0003, -0.0003, 0., 0., 0.])
        .unwrap();
    close(stress[0], 210e6);
    for s in &stress[1..] {
        assert!(s.abs() < 1e-6);
    }
    let hydro = law.stress_pa([0.001, 0.001, 0.001, 0., 0., 0.]).unwrap();
    for s in &hydro[..3] {
        close(*s, 525e6);
    }
    for i in 3..6 {
        let mut strain = [0.; 6];
        strain[i] = 0.002;
        let s = law.stress_pa(strain).unwrap();
        for (j, v) in s.iter().enumerate() {
            close(*v, if i == j { 161_538_461.53846154 } else { 0. });
        }
    }
}

#[test]
fn auxetic_and_nearly_incompressible_materials_keep_positive_energy() {
    for nu in [-0.9, -0.5, 0., 0.3, 0.49, 0.499999] {
        let m = material(10., nu, 1.);
        let law = m
            .isotropic_elasticity(Filter::default(), Filter::default())
            .unwrap();
        for strain in [
            [0.001, 0.002, -0.003, 0.004, 0.005, -0.006],
            [0.001; 6],
            [-0.001; 6],
        ] {
            let stress = law.stress_pa(strain).unwrap();
            assert!(stress.iter().zip(strain).map(|(s, e)| s * e).sum::<f64>() > 0.);
        }
    }
}

#[test]
fn isotropic_response_rotates_with_the_strain_tensor() {
    let m = material(70., 0.25, 10.);
    let law = m
        .isotropic_elasticity(Filter::default(), Filter::default())
        .unwrap();
    // Rotate a uniaxial extension with free contraction 45 degrees about Z.
    let stress = law
        .stress_pa([0.000375, 0.000375, -0.00025, 0., 0., 0.00125])
        .unwrap();
    for (actual, expected) in stress.into_iter().zip([35e6, 35e6, 0., 0., 0., 35e6]) {
        assert!((actual - expected).abs() < 1e-6);
    }
}

#[test]
fn fourier_flux_has_correct_direction_units_and_insulator_limit() {
    let m = material(1., 0.2, 2.);
    let law = m.isotropic_conduction(Filter::default()).unwrap();
    // A 10 K drop over 0.1 m produces 200 W/m2 in the positive X direction.
    assert_eq!(
        law.heat_flux_w_m2([-100., 50., 0.]).unwrap(),
        [200., -100., 0.]
    );
    let mut equivalent = m.clone();
    equivalent.properties[2].value = MaterialValue::Number(0.002);
    equivalent.properties[2].unit = "W/(mm*K)".into();
    assert_eq!(
        equivalent
            .isotropic_conduction(Filter::default())
            .unwrap()
            .heat_flux_w_m2([-100., 50., 0.])
            .unwrap(),
        [200., -100., 0.]
    );
    let insulator = material(1., 0.2, 0.);
    assert_eq!(
        insulator
            .isotropic_conduction(Filter::default())
            .unwrap()
            .heat_flux_w_m2([100.; 3])
            .unwrap(),
        [0.; 3]
    );
}

#[test]
fn invalid_bounds_units_and_nonfinite_values_fail_without_mutation() {
    for (e, nu, k) in [
        (0., 0.3, 1.),
        (-1., 0.3, 1.),
        (1., -1., 1.),
        (1., 0.5, 1.),
        (f64::NAN, 0.3, 1.),
        (1., f64::INFINITY, 1.),
        (f64::MAX, 0.3, 1.),
    ] {
        let m = material(e, nu, k);
        assert!(m
            .isotropic_elasticity(Filter::default(), Filter::default())
            .is_err());
    }
    for k in [-1., f64::NAN, f64::INFINITY] {
        assert!(material(1., 0.3, k)
            .isotropic_conduction(Filter::default())
            .is_err());
    }
    let mut m = material(1., 0.3, 1.);
    m.properties[0].unit = "kg/m^3".into();
    let before = m.clone();
    assert!(m
        .isotropic_elasticity(Filter::default(), Filter::default())
        .is_err());
    assert_eq!(m, before);
    m.properties[0].unit = "Pa".into();
    m.properties[1].value = MaterialValue::Text("temperature dependent".into());
    assert!(m
        .isotropic_elasticity(Filter::default(), Filter::default())
        .is_err());
}

#[test]
fn missing_ambiguous_or_broken_provenance_never_selects_a_default() {
    let mut m = material(70., 0.33, 167.);
    m.properties.push(m.properties[0].clone());
    m.properties[3].context = "Different processing".into();
    assert!(m
        .isotropic_elasticity(Filter::default(), Filter::default())
        .unwrap_err()
        .contains("ambiguous"));
    let choice = Filter {
        context: Some("Synthetic"),
        source_id: Some("fixture"),
    };
    let law = m.isotropic_elasticity(choice, choice).unwrap();
    assert!(std::ptr::eq(law.youngs_property(), &m.properties[0]));
    assert!(std::ptr::eq(law.poisson_property(), &m.properties[1]));
    assert!(std::ptr::eq(
        m.isotropic_conduction(choice)
            .unwrap()
            .conductivity_property(),
        &m.properties[2]
    ));
    m.properties[3].source_id = "missing".into();
    assert!(m.isotropic_elasticity(choice, choice).is_err());
    m.properties.clear();
    assert!(m.isotropic_conduction(Filter::default()).is_err());
}

#[test]
fn saved_material_and_equivalent_units_keep_response_and_provenance() {
    let m = material(70., 0.25, 10.);
    let before = serde_json::to_string(&m).unwrap();
    let reopened: MaterialDetails = serde_json::from_str(&before).unwrap();
    let mut equivalent = reopened.clone();
    equivalent.properties[0].value = MaterialValue::Number(70000.);
    equivalent.properties[0].unit = "MPa".into();
    equivalent.properties[1].value = MaterialValue::Number(25.);
    equivalent.properties[1].unit = "%".into();
    let strain = [0.001, 0., 0., 0.002, 0., 0.];
    assert_eq!(
        m.isotropic_elasticity(Filter::default(), Filter::default())
            .unwrap()
            .stress_pa(strain)
            .unwrap(),
        equivalent
            .isotropic_elasticity(Filter::default(), Filter::default())
            .unwrap()
            .stress_pa(strain)
            .unwrap()
    );
    assert_eq!(serde_json::to_string(&m).unwrap(), before);
    assert_eq!(reopened, m);
}

#[test]
fn response_overflow_and_invalid_gradients_are_explicit_errors() {
    let m = material(210., 0.3, 167.);
    let law = m
        .isotropic_elasticity(Filter::default(), Filter::default())
        .unwrap();
    let heat = m.isotropic_conduction(Filter::default()).unwrap();
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
        assert!(law.stress_pa([v; 6]).is_err());
        assert!(heat.heat_flux_w_m2([v; 3]).is_err());
    }
    let mut tiny = material(1., 0., 1.);
    tiny.properties[0].unit = "Pa".into();
    tiny.properties[0].value = MaterialValue::Number(f64::from_bits(1));
    assert!(tiny
        .isotropic_elasticity(Filter::default(), Filter::default())
        .is_err());
}
