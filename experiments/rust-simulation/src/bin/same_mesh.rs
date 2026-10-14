//! Static elasticity on the exact Rapier cuboid Tet4 topology, in mm/N/MPa.
//! Separates spatial discretization from time-integration/iteration diagnostics.
use anyhow::{ensure, Context, Result};
use faer::{linalg::solvers::Solve, sparse::SparseColMat, sparse::Triplet, Mat, Side};
use fenris::{
    assembly::{
        global::CsrAssembler,
        local::{ElementEllipticAssemblerBuilder, UniformQuadratureTable},
    },
    connectivity::Tet4Connectivity,
    mesh::Tet4Mesh,
    nalgebra::{DVector, Point3},
    nalgebra_sparse::CsrMatrix,
    quadrature,
};
use fenris_solid::{
    materials::{LameParameters, LinearElasticMaterial, YoungPoisson},
    MaterialEllipticOperator,
};
use rapier3d_f64::prelude::*;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Instant};

const FORCE: f64 = 0.1;
const REFERENCE_MM: f64 = 0.26666666666666666;

#[derive(Deserialize)]
struct WorldReport {
    schema: u32,
    units: String,
    library: String,
    material: String,
    beam_load: String,
    beam: Vec<BeamRecord>,
}

#[derive(Deserialize)]
struct BeamRecord {
    status: String,
    result: Option<RapierSample>,
}

#[derive(Deserialize, Serialize)]
struct RapierSample {
    grid: [usize; 3],
    nodes: usize,
    tetrahedra: usize,
    dt_s: f64,
    mean_tip_displacement_mm: f64,
    euler_bernoulli_mm: f64,
    max_particle_speed_m_s: f64,
    final_window_tip_change_mm: f64,
    near_stationary: bool,
    steps: usize,
    linear_tolerance: f64,
    max_linear_iterations: usize,
    max_direct_dofs: usize,
}

fn comparison_sample(report: &WorldReport) -> Result<&RapierSample> {
    ensure!(report.schema == 1, "unsupported world report schema");
    ensure!(
        report.units == "SI internally; reported displacements mm",
        "world report units differ"
    );
    ensure!(
        report.library == "rapier3d-f64 0.36.1, fem + enhanced-determinism",
        "world report library differs"
    );
    ensure!(report.material == "synthetic isotropic E=2000 MPa, nu=0.35; density=1270 kg/m^3; not Bambu PETG calibration", "world report material differs");
    ensure!(report.beam_load == "40x6x2 mm, root clamped, uniform tip traction integrated on linear boundary triangles; total 0.1 N; no contact; sleeping disabled", "world report geometry/load/supports differ");
    let candidates = report
        .beam
        .iter()
        .filter_map(|record| {
            let sample = record.result.as_ref()?;
            (record.status == "completed"
                && sample.grid == [21, 4, 3]
                && (sample.dt_s - 1.0 / 1200.0).abs() < 1e-15
                && sample.max_linear_iterations == 2000
                && sample.max_direct_dofs == 0)
                .then_some(sample)
        })
        .collect::<Vec<_>>();
    ensure!(
        candidates.len() == 1,
        "require exactly one matching completed Rapier comparison case"
    );
    let sample = candidates[0];
    ensure!(
        sample.nodes == 252 && sample.tetrahedra == 600 && sample.steps == 600,
        "Rapier comparison topology or simulation duration differs"
    );
    ensure!(
        (sample.linear_tolerance - 1e-8).abs() < 1e-18,
        "Rapier comparison tolerance differs"
    );
    ensure!(
        (sample.euler_bernoulli_mm - REFERENCE_MM).abs() < 1e-12,
        "Rapier beam reference differs"
    );
    ensure!(
        sample.near_stationary
            && sample.max_particle_speed_m_s.is_finite()
            && sample.max_particle_speed_m_s < 1e-4
            && sample.max_particle_speed_m_s >= 0.0
            && sample.final_window_tip_change_mm.is_finite()
            && sample.final_window_tip_change_mm < 0.001
            && sample.final_window_tip_change_mm >= 0.0,
        "Rapier comparison is not independently near-stationary"
    );
    ensure!(
        sample.mean_tip_displacement_mm.is_finite() && sample.mean_tip_displacement_mm > 0.0,
        "Rapier comparison displacement must be finite and positive"
    );
    Ok(sample)
}

#[derive(Serialize)]
struct Sample {
    grid: [usize; 3],
    nodes: usize,
    tetrahedra: usize,
    free_dofs: usize,
    tip_area_mm2: f64,
    mesh_volume_mm3: f64,
    tip_area_mean_z_mm: f64,
    euler_bernoulli_relative_error: f64,
    free_equilibrium_relative_residual: f64,
    root_reaction_n: [f64; 3],
    root_reaction_moment_n_mm: [f64; 3],
    strain_energy_n_mm: f64,
    half_external_work_n_mm: f64,
    elapsed_seconds: f64,
}

fn norm(values: &[f64]) -> f64 {
    values.iter().map(|v| v * v).sum::<f64>().sqrt()
}

fn product(matrix: &CsrMatrix<f64>, values: &[f64]) -> Vec<f64> {
    let mut result = vec![0.0; matrix.nrows()];
    for (row, col, value) in matrix.triplet_iter() {
        result[row] += value * values[col];
    }
    result
}

fn run(grid: [usize; 3]) -> Result<Sample> {
    let started = Instant::now();
    let [nx, ny, nz] = grid;
    ensure!(nx * ny * nz <= 10_000, "bounded mesh node budget");
    // Identical source geometry/topology to world.rs; stiffness does not depend
    // on Rapier particle mass, radius, damping or its transient solver options.
    let mut world = PhysicsWorld::default();
    let handle = world.insert_soft_body(SoftBodyBuilder::cuboid(
        Vector::new(0.020, 0.0, 0.0),
        Vector::new(0.020, 0.003, 0.001),
        nx,
        ny,
        nz,
    ));
    let body = &world.soft_bodies[handle];
    let vertices = body
        .particle_positions()
        .map(|p| Point3::new(p.x * 1000.0, p.y * 1000.0, p.z * 1000.0))
        .collect::<Vec<_>>();
    let connectivity = body
        .cells()
        .iter()
        .map(|c| Tet4Connectivity(c.vertices.map(|v| v as usize)))
        .collect::<Vec<_>>();
    let volume = body
        .cells()
        .iter()
        .map(|c| c.rest_volume * 1e9)
        .sum::<f64>();
    ensure!(body.cells().iter().all(|c| c.rest_volume > 0.0));
    ensure!(
        (volume - 40.0 * 6.0 * 2.0).abs() < 1e-8,
        "cell volumes must fill cuboid"
    );
    let mesh = Tet4Mesh::from_vertices_and_connectivity(vertices, connectivity);
    let vertices = mesh.vertices();
    let mut weights = vec![0.0; vertices.len()];
    for face in body.boundary() {
        let [a, b, c] = face.map(|i| vertices[i as usize]);
        if [a, b, c].iter().all(|p| (p.x - 40.0).abs() < 1e-10) {
            let area = (b - a).cross(&(c - a)).norm() * 0.5;
            for &i in face {
                weights[i as usize] += area / 3.0;
            }
        }
    }
    let area = weights.iter().sum::<f64>();
    ensure!((area - 12.0).abs() < 1e-10, "tip area mismatch");
    let force = (0..3 * vertices.len())
        .map(|i| {
            if i % 3 == 2 {
                FORCE * weights[i / 3] / area
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();
    ensure!((force.iter().sum::<f64>() - FORCE).abs() < 1e-12);
    let rule = UniformQuadratureTable::from_quadrature_and_uniform_data(
        quadrature::total_order::tetrahedron(1)?,
        LameParameters::from(YoungPoisson {
            young: 2000.0,
            poisson: 0.35,
        }),
    );
    let material = LinearElasticMaterial;
    let operator = MaterialEllipticOperator::new(&material);
    let zero = DVector::zeros(force.len());
    let local = ElementEllipticAssemblerBuilder::new()
        .with_finite_element_space(&mesh)
        .with_operator(&operator)
        .with_quadrature_table(&rule)
        .with_u(&zero)
        .build();
    let stiffness = CsrAssembler::default()
        .assemble(&local)
        .map_err(|error| anyhow::anyhow!("Fenris Tet4 elasticity assembly: {error}"))?;
    ensure!(stiffness.values().iter().all(|v| v.is_finite()));
    let mut count = 0;
    let free = vertices
        .iter()
        .flat_map(|p| (0..3).map(move |_| p.x.abs() < 1e-10))
        .map(|fixed| {
            if fixed {
                None
            } else {
                let index = count;
                count += 1;
                Some(index)
            }
        })
        .collect::<Vec<_>>();
    let triplets = stiffness
        .triplet_iter()
        .filter_map(|(r, c, &v)| Some(Triplet::new(free[r]?, free[c]?, v)))
        .collect::<Vec<_>>();
    let reduced = SparseColMat::try_new_from_triplets(count, count, &triplets)
        .context("convert Tet4 stiffness CSR to CSC")?;
    let mut solution = Mat::zeros(count, 1);
    for (i, mapped) in free.iter().enumerate() {
        if let Some(mapped) = mapped {
            solution[(*mapped, 0)] = force[i];
        }
    }
    reduced
        .sp_cholesky(Side::Lower)
        .context("clamped Tet4 beam must have positive definite stiffness")?
        .solve_in_place(&mut solution);
    let displacement = free
        .iter()
        .map(|i| i.map_or(0.0, |i| solution[(i, 0)]))
        .collect::<Vec<_>>();
    ensure!(displacement.iter().all(|v| v.is_finite()));
    let internal = product(&stiffness, &displacement);
    let residual = internal
        .iter()
        .zip(&force)
        .map(|(a, b)| a - b)
        .collect::<Vec<_>>();
    let free_residual = residual
        .iter()
        .zip(&free)
        .filter_map(|(r, i)| i.map(|_| *r))
        .collect::<Vec<_>>();
    let relative_residual = norm(&free_residual) / norm(&force);
    ensure!(
        relative_residual < 1e-7,
        "free equilibrium failed: {relative_residual}"
    );
    let mut reaction = [0.0; 3];
    let mut moment = [0.0; 3];
    for (i, p) in vertices
        .iter()
        .enumerate()
        .filter(|(_, p)| p.x.abs() < 1e-10)
    {
        let r = &residual[3 * i..3 * i + 3];
        for axis in 0..3 {
            reaction[axis] += r[axis];
        }
        moment[0] += p.y * r[2] - p.z * r[1];
        moment[1] += p.z * r[0] - p.x * r[2];
        moment[2] += p.x * r[1] - p.y * r[0];
    }
    ensure!(
        (reaction[2] + FORCE).abs() < 1e-8 && reaction[0].abs() < 1e-8 && reaction[1].abs() < 1e-8,
        "root force balance failed"
    );
    ensure!(
        (moment[1] - 4.0).abs() < 1e-7 && moment[0].abs() < 1e-7 && moment[2].abs() < 1e-7,
        "root moment balance failed"
    );
    let energy = 0.5
        * displacement
            .iter()
            .zip(&internal)
            .map(|(u, f)| u * f)
            .sum::<f64>();
    let work = 0.5
        * displacement
            .iter()
            .zip(&force)
            .map(|(u, f)| u * f)
            .sum::<f64>();
    ensure!(
        energy > 0.0 && (energy - work).abs() / energy < 1e-7,
        "energy/work balance failed"
    );
    let tip = 2.0 * work / FORCE;
    Ok(Sample {
        grid,
        nodes: vertices.len(),
        tetrahedra: mesh.connectivity().len(),
        free_dofs: count,
        tip_area_mm2: area,
        mesh_volume_mm3: volume,
        tip_area_mean_z_mm: tip,
        euler_bernoulli_relative_error: tip / REFERENCE_MM - 1.0,
        free_equilibrium_relative_residual: relative_residual,
        root_reaction_n: reaction,
        root_reaction_moment_n_mm: moment,
        strain_energy_n_mm: energy,
        half_external_work_n_mm: work,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    })
}

fn main() -> Result<()> {
    let world_path = std::env::args_os()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("world-results.json"));
    let source = std::fs::read(&world_path)
        .with_context(|| format!("read Rapier comparison {}", world_path.display()))?;
    let world_report: WorldReport =
        serde_json::from_slice(&source).context("parse Rapier world report")?;
    let rapier = comparison_sample(&world_report)?;
    let samples = [[21, 4, 3], [41, 4, 3], [21, 7, 5], [41, 7, 5], [81, 13, 9]]
        .into_iter()
        .map(run)
        .collect::<Result<Vec<_>>>()?;
    let coarse = samples[0].tip_area_mean_z_mm;
    let difference = rapier.mean_tip_displacement_mm / coarse - 1.0;
    // This is a narrow cross-library agreement gate for the existing 2000-PCG
    // near-stationary trajectory, not a certification of Rapier contact/stress.
    ensure!(
        difference.abs() < 0.02,
        "same-mesh Rapier/Fenris displacement disagreement exceeds 2%: {difference}"
    );
    let report = serde_json::json!({
        "schema": 1,
        "experiment": "Fenris Tet4 linear static on exact Rapier cuboid rest topology",
        "versions": {"fenris": "0.0.33", "fenris_solid": "0.0.33", "faer": "0.24.4", "rapier3d_f64": "0.36.1"},
        "geometry_mm": [40.0, 6.0, 2.0],
        "material": {"young_mpa": 2000.0, "poisson": 0.35, "status": "synthetic isotropic benchmark; not printed PETG calibration"},
        "boundary_conditions": "all X=0 root translations fixed; consistent uniform tip traction in +Z; Rapier comparison has opposite sign, equal magnitude",
        "load_n": FORCE, "euler_bernoulli_tip_mm": REFERENCE_MM,
        "quadrature": "one point; exact for constant linear-Tet4 elasticity integrand",
        "samples": samples,
        "rapier_comparison": {"source": world_path, "source_sample": rapier, "tip_displacement_mm": rapier.mean_tip_displacement_mm, "same_mesh_relative_difference": difference, "agreement_gate": 0.02},
        "limits": "Linear static and corotational dynamic formulations differ; this comparison tests one settled low-load case. Refinement diagnoses spatial bending stiffness error but does not uniquely prove shear locking. No clip contact/preload, stress, creep, anisotropy or manufactured-part qualification. Finest Tet4 mesh is not assumed converged.",
        "timing_scope": "mesh extraction, assembly, sparse solve and equilibrium checks",
    });
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("same-mesh-results.json"));
    std::fs::write(&output, serde_json::to_string_pretty(&report)? + "\n")
        .with_context(|| format!("write {}", output.display()))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
