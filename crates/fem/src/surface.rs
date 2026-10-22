use crate::{element::face_area, ThermalMesh, ThermalSurface};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct SurfaceAssembly {
    pub areas: Vec<f64>,
    pub anchored: Vec<bool>,
}

pub(crate) fn assemble(
    mesh: &ThermalMesh,
    surfaces: &[ThermalSurface],
    rows: &mut [BTreeMap<usize, f64>],
    load: &mut [f64],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<SurfaceAssembly, String> {
    // Called only after every tetrahedron and node index has been validated.
    let mut faces = BTreeMap::<[usize; 3], u8>::new();
    for e in &mesh.elements {
        if cancelled() {
            return Err("Thermal solve cancelled".into());
        }
        for local in [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
            let mut face = local.map(|i| e.nodes[i]);
            face.sort_unstable();
            let count = faces.entry(face).or_default();
            *count += 1;
            if *count > 2 {
                return Err("Nonmanifold thermal mesh face".into());
            }
        }
    }
    let mut used = BTreeSet::new();
    let mut areas = Vec::with_capacity(surfaces.len());
    let mut anchored = vec![false; mesh.points_m.len()];
    for s in surfaces {
        if cancelled() {
            return Err("Thermal solve cancelled".into());
        }
        let mut face = s.nodes;
        face.sort_unstable();
        if faces.get(&face) != Some(&1) || !used.insert(face) {
            return Err("Thermal surface must be a unique exterior mesh face".into());
        }
        if !s.heat_flux_in_w_m2.is_finite() {
            return Err("Thermal surface flux must be finite".into());
        }
        let (h, ambient) = s
            .convection
            .map(|c| (c.coefficient_w_m2_k, c.ambient_k))
            .unwrap_or((0., 0.));
        if !h.is_finite() || h < 0. || !ambient.is_finite() || ambient < 0. {
            return Err("Invalid thermal convection coefficient or ambient temperature".into());
        }
        let area = face_area(s.nodes.map(|i| mesh.points_m[i]))?;
        let coefficient = h * area / 12.;
        if !coefficient.is_finite() || (h > 0. && coefficient <= 0.) {
            return Err("Thermal convection exceeds representable coefficient range".into());
        }
        for (i, &node) in s.nodes.iter().enumerate() {
            anchored[node] |= h > 0.;
            load[node] += (s.heat_flux_in_w_m2 + h * ambient) * area / 3.;
            for (j, &other) in s.nodes.iter().enumerate() {
                *rows[node].entry(other).or_default() +=
                    coefficient * (if i == j { 2. } else { 1. });
            }
        }
        areas.push(area);
    }
    Ok(SurfaceAssembly { areas, anchored })
}

pub(crate) fn heat(
    surfaces: &[ThermalSurface],
    areas: &[f64],
    temperatures: &[f64],
) -> Result<Vec<f64>, String> {
    surfaces
        .iter()
        .zip(areas)
        .map(|(s, area)| {
            let average = s.nodes.iter().map(|i| temperatures[*i] / 3.).sum::<f64>();
            let convection = s
                .convection
                .map(|c| c.coefficient_w_m2_k * (c.ambient_k - average))
                .unwrap_or(0.);
            let power = (s.heat_flux_in_w_m2 + convection) * area;
            if power.is_finite() {
                Ok(power)
            } else {
                Err("Thermal surface heat exceeds numeric range".into())
            }
        })
        .collect()
}
