//! Explicit, small-strain isotropic material laws over the shared catalog.
//! These are local constitutive responses, not a mesh or boundary-value solver.
//! Selecting a model is a caller decision; material family never implies that
//! isotropy, linearity, temperature independence or a load regime is qualified.
use crate::{MaterialDetails, MaterialProperty, PhysicalDimension};

/// Exact optional filters for an existing property role. Unresolved ambiguity
/// is an error even when candidate values happen to agree.
#[derive(Debug, Clone, Copy, Default)]
pub struct MaterialPropertyFilter<'a> {
    pub context: Option<&'a str>,
    pub source_id: Option<&'a str>,
}

/// Borrowed inputs keep the original values, units, context and source identity
/// available alongside derived SI constants. No second catalog or saved model.
#[derive(Debug)]
pub struct IsotropicElasticity<'a> {
    youngs: &'a MaterialProperty,
    poisson: &'a MaterialProperty,
    youngs_pa: f64,
    poisson_ratio: f64,
    bulk_pa: f64,
    shear_pa: f64,
}

#[derive(Debug)]
pub struct IsotropicConduction<'a> {
    conductivity: &'a MaterialProperty,
    conductivity_w_m_k: f64,
}

impl MaterialDetails {
    /// Explicitly select constant isotropic elasticity using YoungsModulus and
    /// PoissonRatio. Does not infer applicability from kind or silently combine
    /// competing engineering/print-profile records.
    pub fn isotropic_elasticity(
        &self,
        youngs: MaterialPropertyFilter<'_>,
        poisson: MaterialPropertyFilter<'_>,
    ) -> Result<IsotropicElasticity<'_>, String> {
        let youngs = self.select_property("YoungsModulus", youngs.context, youngs.source_id)?;
        let poisson = self.select_property("PoissonRatio", poisson.context, poisson.source_id)?;
        let youngs_pa = youngs.value_si(PhysicalDimension::Pressure)?;
        let poisson_ratio = poisson.value_si(PhysicalDimension::Dimensionless)?;
        if youngs_pa <= 0. || !(-1. < poisson_ratio && poisson_ratio < 0.5) {
            return Err(
                "Isotropic elasticity requires YoungsModulus > 0 and -1 < PoissonRatio < 0.5"
                    .into(),
            );
        }
        let bulk_pa = youngs_pa / (3. * (1. - 2. * poisson_ratio));
        let shear_pa = youngs_pa / (2. * (1. + poisson_ratio));
        if ![bulk_pa, shear_pa].iter().all(|v| v.is_finite() && *v > 0.) {
            return Err("Derived isotropic moduli exceed the finite positive numeric range".into());
        }
        Ok(IsotropicElasticity {
            youngs,
            poisson,
            youngs_pa,
            poisson_ratio,
            bulk_pa,
            shear_pa,
        })
    }

    /// Explicitly select a constant scalar Fourier conduction law. Zero is
    /// allowed as an ideal insulator; negative conductivity is non-passive.
    pub fn isotropic_conduction(
        &self,
        selection: MaterialPropertyFilter<'_>,
    ) -> Result<IsotropicConduction<'_>, String> {
        let conductivity = self.select_property(
            "ThermalConductivity",
            selection.context,
            selection.source_id,
        )?;
        let conductivity_w_m_k = conductivity.value_si(PhysicalDimension::ThermalConductivity)?;
        if conductivity_w_m_k < 0. {
            return Err("Isotropic thermal conductivity must be nonnegative".into());
        }
        Ok(IsotropicConduction {
            conductivity,
            conductivity_w_m_k,
        })
    }
}

impl<'a> IsotropicElasticity<'a> {
    pub fn youngs_property(&self) -> &'a MaterialProperty {
        self.youngs
    }
    pub fn poisson_property(&self) -> &'a MaterialProperty {
        self.poisson
    }
    pub fn youngs_modulus_pa(&self) -> f64 {
        self.youngs_pa
    }
    pub fn poisson_ratio(&self) -> f64 {
        self.poisson_ratio
    }
    pub fn bulk_modulus_pa(&self) -> f64 {
        self.bulk_pa
    }
    pub fn shear_modulus_pa(&self) -> f64 {
        self.shear_pa
    }

    /// 3D stress [xx, yy, zz, yz, xz, xy] in Pa from engineering strain
    /// [exx, eyy, ezz, gamma_yz, gamma_xz, gamma_xy]. Engineering shear is
    /// twice tensor shear. This is neither a plane-stress reduction nor a
    /// finite-strain/thermal-expansion update.
    pub fn stress_pa(&self, strain: [f64; 6]) -> Result<[f64; 6], String> {
        finite(&strain, "Engineering strain")?;
        let trace = strain[0] + strain[1] + strain[2];
        let pressure = self.bulk_pa * trace;
        let mut stress = [0.; 6];
        for i in 0..3 {
            stress[i] = pressure + self.shear_pa * (2. * (strain[i] - trace / 3.));
            stress[i + 3] = self.shear_pa * strain[i + 3];
        }
        finite(&stress, "Constitutive stress")?;
        Ok(stress)
    }
}

impl<'a> IsotropicConduction<'a> {
    pub fn conductivity_property(&self) -> &'a MaterialProperty {
        self.conductivity
    }
    pub fn conductivity_w_m_k(&self) -> f64 {
        self.conductivity_w_m_k
    }

    /// Fourier heat flux [x,y,z] in W/m^2 from a temperature gradient in K/m.
    /// Absolute Celsius/Kelvin offsets do not enter a gradient.
    pub fn heat_flux_w_m2(&self, gradient_k_m: [f64; 3]) -> Result<[f64; 3], String> {
        finite(&gradient_k_m, "Temperature gradient")?;
        let flux = gradient_k_m.map(|v| -self.conductivity_w_m_k * v);
        finite(&flux, "Constitutive heat flux")?;
        Ok(flux)
    }
}

fn finite<const N: usize>(values: &[f64; N], label: &str) -> Result<(), String> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(format!("{label} must be finite and within numeric range"))
    }
}
