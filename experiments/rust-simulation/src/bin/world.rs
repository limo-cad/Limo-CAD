//! Reproducible SI-unit smoke experiments, not a printed-material calibration.
use anyhow::{ensure, Result};
use rapier3d_f64::prelude::*;
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Serialize)]
struct MotionResult {
    dt_s: f64,
    free_fall_error_mm: f64,
    settled_height_error_mm: f64,
    settled_speed_m_s: f64,
}

fn motion(dt: f64) -> Result<MotionResult> {
    let mut world = PhysicsWorld::default();
    world.integration_parameters.dt = dt;
    let (ball, _) = world.insert(
        RigidBodyBuilder::dynamic().translation(Vector::new(0.0, 10.0, 0.0)),
        ColliderBuilder::ball(0.5).restitution(0.0),
    );
    for _ in 0..(1.0 / dt).round() as usize {
        world.step();
    }
    let free_fall_error_mm = (world.bodies[ball].translation().y - (10.0 - 0.5 * 9.81)) * 1000.0;
    ensure!(free_fall_error_mm.is_finite(), "non-finite free fall");
    world.insert(
        RigidBodyBuilder::fixed().translation(Vector::new(0.0, -0.1, 0.0)),
        ColliderBuilder::cuboid(20.0, 0.1, 20.0).restitution(0.0),
    );
    for _ in 0..(4.0 / dt).round() as usize {
        world.step();
    }
    let height_error = (world.bodies[ball].translation().y - 0.5) * 1000.0;
    let speed = world.bodies[ball].linvel().length();
    ensure!(
        height_error.is_finite() && speed.is_finite(),
        "non-finite contact"
    );
    ensure!(
        height_error.abs() < 5.0 && speed < 1e-3,
        "contact did not settle"
    );
    Ok(MotionResult {
        dt_s: dt,
        free_fall_error_mm,
        settled_height_error_mm: height_error,
        settled_speed_m_s: speed,
    })
}

#[derive(Serialize)]
struct BeamResult {
    grid: [usize; 3],
    nodes: usize,
    tetrahedra: usize,
    dt_s: f64,
    elapsed_s: f64,
    mean_tip_displacement_mm: f64,
    euler_bernoulli_mm: f64,
    reference_error_percent: f64,
    max_tip_speed_m_s: f64,
    max_particle_speed_m_s: f64,
    final_window_tip_change_mm: f64,
    near_stationary: bool,
    steps: usize,
    linear_tolerance: f64,
    max_linear_iterations: usize,
    max_direct_dofs: usize,
}

fn beam(
    grid: [usize; 3],
    dt: f64,
    max_direct_dofs: usize,
    max_linear_iterations: usize,
) -> Result<BeamResult> {
    let started = Instant::now();
    let [nx, ny, nz] = grid;
    let mut world = PhysicsWorld {
        gravity: Vector::ZERO,
        ..PhysicsWorld::default()
    };
    world.integration_parameters.dt = dt;
    world
        .integration_parameters
        .soft_bodies
        .fem
        .linear_tolerance = 1e-8;
    world
        .integration_parameters
        .soft_bodies
        .fem
        .max_linear_iterations = max_linear_iterations;
    // This controls constraint responses; the elasticity predictor still uses PCG.
    world.integration_parameters.soft_bodies.fem.max_dense_dofs = max_direct_dofs;
    let builder = SoftBodyBuilder::cuboid(
        Vector::new(0.020, 0.0, 0.0),
        Vector::new(0.020, 0.003, 0.001),
        nx,
        ny,
        nz,
    )
    .solver(SoftBodySolver::Fem)
    .cell_model(SoftBodyCellModel::Corotational)
    .edges(vec![])
    .shape_matching(false)
    .can_sleep(false)
    .volume_preservation(false)
    .particle_radius(0.00005)
    .mass(1270.0 * 0.040 * 0.006 * 0.002)
    .pinned_particles(0..(ny * nz) as u32)
    .material(SoftBodyMaterial {
        young_modulus: 2e9,
        poisson_ratio: 0.35,
        deformation_damping: 40.0,
        ..Default::default()
    });
    let handle = world.insert_soft_body(builder);
    let first_tip = (nx - 1) * ny * nz;
    let tip = first_tip..nx * ny * nz;
    let mut weights = vec![0.0; nx * ny * nz];
    let body = &world.soft_bodies[handle];
    for face in body.boundary() {
        let [a, b, c] = face.map(|i| body.particle_position(i as usize));
        if [a, b, c].iter().all(|p| (p.x - 0.040).abs() < 1e-12) {
            let area = (b - a).cross(c - a).length() * 0.5;
            for &i in face {
                weights[i as usize] += area / 3.0;
            }
        }
    }
    let tip_area: f64 = weights.iter().sum();
    ensure!(
        (tip_area - 0.006 * 0.002).abs() < 1e-12,
        "tip area mismatch"
    );
    for i in tip.clone() {
        world.soft_bodies[handle].add_particle_force(
            i,
            Vector::new(0.0, 0.0, -0.1 * weights[i] / tip_area),
            true,
        );
    }
    let steps = (0.5 / dt).round() as usize;
    ensure!(steps <= 1200, "step budget exceeded");
    let mut final_window = Vec::new();
    for step in 0..steps {
        ensure!(
            started.elapsed() < Duration::from_secs(45),
            "beam time budget exceeded"
        );
        world.step();
        ensure!(
            world.soft_bodies[handle]
                .particle_positions()
                .all(|p| p.is_finite()),
            "non-finite beam"
        );
        ensure!(
            (0..nx * ny * nz).all(|i| world.soft_bodies[handle].particle_velocity(i).is_finite()),
            "non-finite beam velocity"
        );
        if step >= steps * 9 / 10 {
            final_window.push(
                tip.clone()
                    .map(|i| weights[i] * world.soft_bodies[handle].particle_position(i).z)
                    .sum::<f64>()
                    / tip_area
                    * 1000.0,
            );
        }
    }
    let body = &world.soft_bodies[handle];
    let mean_tip = tip
        .clone()
        .map(|i| weights[i] * body.particle_position(i).z)
        .sum::<f64>()
        / tip_area;
    let max_speed = tip
        .map(|i| body.particle_velocity(i).length())
        .fold(0.0, f64::max);
    let max_particle_speed = (0..body.num_particles())
        .map(|i| body.particle_velocity(i).length())
        .fold(0.0, f64::max);
    let window_change = final_window
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
        - final_window.iter().copied().fold(f64::INFINITY, f64::min);
    let reference =
        0.1 * 0.040_f64.powi(3) / (3.0 * 2e9 * (0.006 * 0.002_f64.powi(3) / 12.0)) * 1000.0;
    let displacement = -mean_tip * 1000.0;
    Ok(BeamResult {
        grid,
        nodes: body.num_particles(),
        tetrahedra: body.cells().len(),
        dt_s: dt,
        elapsed_s: started.elapsed().as_secs_f64(),
        mean_tip_displacement_mm: displacement,
        euler_bernoulli_mm: reference,
        reference_error_percent: (displacement / reference - 1.0) * 100.0,
        max_tip_speed_m_s: max_speed,
        max_particle_speed_m_s: max_particle_speed,
        final_window_tip_change_mm: window_change,
        near_stationary: max_particle_speed < 1e-4 && window_change < 0.001,
        steps,
        linear_tolerance: 1e-8,
        max_linear_iterations,
        max_direct_dofs,
    })
}

fn main() -> Result<()> {
    fn record<T: Serialize>(result: Result<T>) -> serde_json::Value {
        match result {
            Ok(value) => serde_json::json!({"status": "completed", "result": value}),
            Err(error) => serde_json::json!({"status": "failed", "error": error.to_string()}),
        }
    }
    let report = serde_json::json!({
        "schema": 1,
        "library": "rapier3d-f64 0.36.1, fem + enhanced-determinism",
        "units": "SI internally; reported displacements mm",
        "material": "synthetic isotropic E=2000 MPa, nu=0.35; density=1270 kg/m^3; not Bambu PETG calibration",
        "beam_load": "40x6x2 mm, root clamped, uniform tip traction integrated on linear boundary triangles; total 0.1 N; no contact; sleeping disabled",
        "motion": [record(motion(1.0 / 120.0)), record(motion(1.0 / 240.0))],
        "beam": [record(beam([21, 4, 3], 1.0 / 600.0, 0, 250)), record(beam([21, 4, 3], 1.0 / 1200.0, 0, 250)), record(beam([41, 4, 3], 1.0 / 600.0, 0, 250)), record(beam([21, 4, 3], 1.0 / 1200.0, 2000, 250)), record(beam([41, 4, 3], 1.0 / 600.0, 2000, 250)), record(beam([21, 4, 3], 1.0 / 1200.0, 0, 2000))],
        "limitations": "Transient settlement screening only; total mass split uniformly across particles. Rapier solver diagnostics/reactions are not checked by this harness. Mesh sweep refines X only; mesh/time sweeps do not establish stress accuracy. Execution success does not gate beam reference error or convergence."
    });
    if let Some(path) = std::env::args_os().nth(1) {
        std::fs::write(path, serde_json::to_string_pretty(&report)? + "\n")?;
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    ensure!(
        report["motion"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(report["beam"].as_array().into_iter().flatten())
            .all(|r| r["status"] == "completed"),
        "one or more experiments failed; see the report"
    );
    Ok(())
}
