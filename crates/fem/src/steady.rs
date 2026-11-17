use crate::{
    element::{dot, geometry},
    SolveOptions, ThermalMesh, ThermalResult, ThermalSurface,
};
use limo_cad_core::IsotropicConduction;
use std::collections::{BTreeMap, BTreeSet};
fn norm(v: &[f64]) -> f64 {
    v.iter().fold(0., |n, x| n.hypot(*x))
}
fn inner(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn multiply(rows: &[BTreeMap<usize, f64>], x: &[f64]) -> Vec<f64> {
    rows.iter()
        .map(|r| r.iter().map(|(j, v)| v * x[*j]).sum())
        .collect()
}
fn check_cancel(cancelled: &mut impl FnMut() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("Thermal solve cancelled".into())
    } else {
        Ok(())
    }
}

/// Solve -div(k grad T)=Q with linear tetrahedra. Unspecified boundary flux
/// is zero. prescribed_k contains unique (node, absolute Kelvin) pairs;
/// nodal_load_w contains integrated nodal heat inputs, in watts.
/// Every connected component needs a temperature constraint. No singularity
/// regularization, inferred constraints, partial result or input mutation.
pub fn solve_steady(
    mesh: &ThermalMesh,
    materials: &[IsotropicConduction<'_>],
    prescribed_k: &[(usize, f64)],
    nodal_load_w: &[f64],
    options: SolveOptions,
    cancelled: impl FnMut() -> bool,
) -> Result<ThermalResult, String> {
    solve_steady_with_surfaces(
        mesh,
        materials,
        prescribed_k,
        nodal_load_w,
        &[],
        options,
        cancelled,
    )
}

/// Include exterior surface flux and constant convection. Each component must
/// have a prescribed temperature or a positive convection coefficient.
pub fn solve_steady_with_surfaces(
    mesh: &ThermalMesh,
    materials: &[IsotropicConduction<'_>],
    prescribed_k: &[(usize, f64)],
    nodal_load_w: &[f64],
    surfaces: &[ThermalSurface],
    options: SolveOptions,
    mut cancelled: impl FnMut() -> bool,
) -> Result<ThermalResult, String> {
    check_cancel(&mut cancelled)?;
    let n = mesh.points_m.len();
    if n == 0
        || mesh.elements.is_empty()
        || n > options.max_nodes
        || mesh.elements.len() > options.max_elements
        || surfaces.len() > mesh.elements.len().saturating_mul(4)
        || options.max_iterations == 0
        || !options.relative_tolerance.is_finite()
        || !(0. < options.relative_tolerance && options.relative_tolerance < 1.)
        || !options.absolute_tolerance_w.is_finite()
        || options.absolute_tolerance_w <= 0.
    {
        return Err("Invalid thermal solve size, budget or tolerance".into());
    }
    if nodal_load_w.len() != n
        || !nodal_load_w.iter().all(|v| v.is_finite())
        || !mesh.points_m.iter().flatten().all(|v| v.is_finite())
    {
        return Err("Thermal points and nodal loads must be finite and correctly sized".into());
    }
    let mut fixed = vec![None; n];
    for &(node, t) in prescribed_k {
        if node >= n || !t.is_finite() || t < 0. || fixed[node].replace(t).is_some() {
            return Err("Invalid or duplicate prescribed temperature".into());
        }
    }
    let mut rows = vec![BTreeMap::<usize, f64>::new(); n];
    let mut load = nodal_load_w.to_vec();
    let mut adjacency = vec![BTreeSet::new(); n];
    let mut seen_elements = BTreeSet::new();
    let mut geometries = Vec::with_capacity(mesh.elements.len());
    for element in &mesh.elements {
        check_cancel(&mut cancelled)?;
        let mut ids = element.nodes;
        ids.sort_unstable();
        if ids[3] >= n
            || ids.windows(2).any(|p| p[0] == p[1])
            || !seen_elements.insert(ids)
            || !element.source_w_m3.is_finite()
        {
            return Err("Invalid, repeated or duplicate thermal tetrahedron".into());
        }
        let material = materials
            .get(element.material)
            .ok_or("Thermal element material is missing")?;
        let k = material.conductivity_w_m_k();
        if k <= 0. {
            return Err("Conductive tetrahedra require positive conductivity; model insulation as a boundary".into());
        }
        let g = geometry(element.nodes.map(|i| mesh.points_m[i]))?;
        for i in 0..4 {
            let row = element.nodes[i];
            load[row] += element.source_w_m3 * g.volume / 4.;
            for j in 0..4 {
                let col = element.nodes[j];
                adjacency[row].insert(col);
                *rows[row].entry(col).or_default() +=
                    k * g.volume * dot(g.gradients[i], g.gradients[j]);
            }
        }
        geometries.push(g);
    }
    let applied_power_w = load.iter().sum::<f64>();
    let surface = crate::surface::assemble(mesh, surfaces, &mut rows, &mut load, &mut cancelled)?;
    if !load.iter().all(|x| x.is_finite())
        || !rows.iter().flat_map(|r| r.values()).all(|x| x.is_finite())
    {
        return Err("Thermal assembly exceeds numeric range".into());
    }
    let mut visited = vec![false; n];
    for start in 0..n {
        if visited[start] {
            continue;
        }
        let mut pending = vec![start];
        let mut anchored = false;
        visited[start] = true;
        while let Some(i) = pending.pop() {
            anchored |= fixed[i].is_some() || surface.anchored[i];
            for &j in &adjacency[i] {
                if !visited[j] {
                    visited[j] = true;
                    pending.push(j);
                }
            }
        }
        if !anchored {
            return Err(
                "Every thermal component requires a prescribed temperature or positive convection"
                    .into(),
            );
        }
    }
    let diagonal: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| r.get(&i).copied().unwrap_or(0.))
        .collect();
    if diagonal.iter().any(|d| !d.is_finite() || *d <= 0.) {
        return Err("Unused node or nonpositive thermal diagonal".into());
    }
    let boundary: Vec<_> = fixed.iter().map(|v| v.unwrap_or(0.)).collect();
    let ab = multiply(&rows, &boundary);
    let rhs: Vec<_> = load
        .iter()
        .zip(ab)
        .enumerate()
        .map(|(i, (f, a))| if fixed[i].is_some() { 0. } else { f - a })
        .collect();
    let tolerance = options
        .absolute_tolerance_w
        .max(options.relative_tolerance * norm(&rhs));
    if !tolerance.is_finite() || !rhs.iter().all(|x| x.is_finite()) {
        return Err("Thermal boundary elimination exceeds numeric range".into());
    }
    let mut x = vec![0.; n];
    let mut residual = rhs.clone();
    let mut z: Vec<_> = residual.iter().zip(&diagonal).map(|(r, d)| r / d).collect();
    let mut direction = z.clone();
    let mut rz = inner(&residual, &z);
    let mut iterations = 0;
    while norm(&residual) > tolerance {
        check_cancel(&mut cancelled)?;
        if iterations >= options.max_iterations {
            return Err("Thermal solver did not converge within iteration budget".into());
        }
        let mut ad = multiply(&rows, &direction);
        for i in 0..n {
            if fixed[i].is_some() {
                ad[i] = 0.;
            }
        }
        let curvature = inner(&direction, &ad);
        if !curvature.is_finite() || curvature <= 0. || !rz.is_finite() || rz <= 0. {
            return Err("Thermal system is singular or numerically ill-conditioned".into());
        }
        let alpha = rz / curvature;
        for i in 0..n {
            x[i] += alpha * direction[i];
            residual[i] -= alpha * ad[i];
        }
        if !x.iter().chain(&residual).all(|v| v.is_finite()) {
            return Err("Thermal iteration exceeds numeric range".into());
        }
        iterations += 1;
        if norm(&residual) <= tolerance {
            break;
        }
        z = residual.iter().zip(&diagonal).map(|(r, d)| r / d).collect();
        let next = inner(&residual, &z);
        let beta = next / rz;
        for i in 0..n {
            direction[i] = z[i] + beta * direction[i];
        }
        rz = next;
    }
    check_cancel(&mut cancelled)?;
    let temperature_k: Vec<_> = x.iter().zip(boundary).map(|(x, b)| x + b).collect();
    if temperature_k.iter().any(|t| !t.is_finite() || *t < 0.) {
        return Err("Solved absolute temperature is negative or nonfinite".into());
    }
    let actual = multiply(&rows, &temperature_k);
    let mut reaction_w = vec![0.; n];
    let mut free_residual = vec![0.; n];
    for i in 0..n {
        let r = actual[i] - load[i];
        if fixed[i].is_some() {
            reaction_w[i] = r;
        } else {
            free_residual[i] = r;
        }
    }
    let free_residual_l2_w = norm(&free_residual);
    if !free_residual_l2_w.is_finite() || free_residual_l2_w > tolerance {
        return Err("Thermal result failed the recomputed residual check".into());
    }
    let surface_heat_w = crate::surface::heat(surfaces, &surface.areas, &temperature_k)?;
    let balance_error_w =
        reaction_w.iter().sum::<f64>() + applied_power_w + surface_heat_w.iter().sum::<f64>();
    let balance_scale = load.iter().chain(&reaction_w).map(|v| v.abs()).sum::<f64>();
    let balance_limit = tolerance * (n as f64).sqrt() + 1e-12 * balance_scale;
    if !balance_error_w.is_finite()
        || !balance_limit.is_finite()
        || balance_error_w.abs() > balance_limit
    {
        return Err("Thermal result failed global heat balance".into());
    }
    let mut element_flux_w_m2 = Vec::with_capacity(mesh.elements.len());
    for (e, g) in mesh.elements.iter().zip(geometries) {
        check_cancel(&mut cancelled)?;
        let gradient = std::array::from_fn(|j| {
            (0..4)
                .map(|i| g.gradients[i][j] * temperature_k[e.nodes[i]])
                .sum()
        });
        element_flux_w_m2.push(materials[e.material].heat_flux_w_m2(gradient)?);
    }
    check_cancel(&mut cancelled)?;
    Ok(ThermalResult {
        temperature_k,
        reaction_w,
        element_flux_w_m2,
        surface_heat_w,
        iterations,
        free_residual_l2_w,
        balance_error_w,
    })
}
