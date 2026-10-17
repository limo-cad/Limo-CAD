use limo_cad_core::{
    MaterialDetails, MaterialProperty, MaterialPropertyFilter, MaterialSource, MaterialValue,
};
use limo_cad_fem::*;
fn material(k: f64) -> MaterialDetails {
    MaterialDetails {
        kind: "other".into(),
        catalog_id: "synthetic.thermal".into(),
        properties: vec![MaterialProperty {
            name: "ThermalConductivity".into(),
            value: MaterialValue::Number(k),
            unit: "W/(m*K)".into(),
            context: "Synthetic".into(),
            source_id: "fixture".into(),
        }],
        sources: vec![MaterialSource {
            id: "fixture".into(),
            repository: "test".into(),
            revision: "1".repeat(40),
            path: "thermal".into(),
            sha256: "2".repeat(64),
            license: "test".into(),
            author: "test".into(),
            reference: String::new(),
        }],
        ..Default::default()
    }
}
fn cube(n: usize) -> ThermalMesh {
    let mut points = Vec::new();
    for z in 0..=n {
        for y in 0..=n {
            for x in 0..=n {
                points.push([
                    x as f64 / n as f64,
                    y as f64 / n as f64,
                    z as f64 / n as f64,
                ]);
            }
        }
    }
    let id = |x, y, z| x + (n + 1) * (y + (n + 1) * z);
    let mut elements = Vec::new();
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                let v: [usize; 8] = std::array::from_fn(|i| {
                    id(x + (i & 1), y + ((i >> 1) & 1), z + ((i >> 2) & 1))
                });
                for nodes in [
                    [0, 1, 3, 7],
                    [0, 3, 2, 7],
                    [0, 2, 6, 7],
                    [0, 6, 4, 7],
                    [0, 4, 5, 7],
                    [0, 5, 1, 7],
                ] {
                    elements.push(ThermalTet {
                        nodes: nodes.map(|i| v[i]),
                        material: 0,
                        source_w_m3: 0.,
                    });
                }
            }
        }
    }
    ThermalMesh {
        points_m: points,
        elements,
    }
}
fn boundary(mesh: &ThermalMesh, f: impl Fn([f64; 3]) -> f64) -> Vec<(usize, f64)> {
    mesh.points_m
        .iter()
        .enumerate()
        .filter(|(_, p)| p.iter().any(|x| *x == 0. || *x == 1.))
        .map(|(i, p)| (i, f(*p)))
        .collect()
}
fn solve(mesh: &ThermalMesh, bc: &[(usize, f64)]) -> ThermalResult {
    let m = material(2.);
    solve_steady(
        mesh,
        &[m.isotropic_conduction(MaterialPropertyFilter::default())
            .unwrap()],
        bc,
        &vec![0.; mesh.points_m.len()],
        SolveOptions::default(),
        || false,
    )
    .unwrap()
}
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() < tol, "{a} != {b}, tolerance {tol}");
}

#[test]
fn affine_temperature_patch_recovers_interior_values_flux_and_reactions() {
    let mesh = cube(3);
    let before = mesh.clone();
    let field = |p: [f64; 3]| 300. + 10. * p[0] - 20. * p[1] + 30. * p[2];
    let r = solve(&mesh, &boundary(&mesh, field));
    assert!(r.iterations > 0);
    for (p, t) in mesh.points_m.iter().zip(&r.temperature_k) {
        close(*t, field(*p), 1e-7);
    }
    for flux in &r.element_flux_w_m2 {
        for (a, b) in flux.iter().zip([-20., 40., -60.]) {
            close(*a, b, 1e-6);
        }
    }
    close(r.reaction_w.iter().sum(), 0., 1e-7);
    assert!(r.free_residual_l2_w < 1e-6);
    assert_eq!(mesh, before);
}

#[test]
fn slab_with_insulated_sides_balances_analytical_heat_rate() {
    let mesh = cube(4);
    let bc: Vec<_> = mesh
        .points_m
        .iter()
        .enumerate()
        .filter(|(_, p)| p[0] == 0. || p[0] == 1.)
        .map(|(i, p)| (i, 300. + 100. * p[0]))
        .collect();
    let r = solve(&mesh, &bc);
    for (p, t) in mesh.points_m.iter().zip(&r.temperature_k) {
        close(*t, 300. + 100. * p[0], 1e-6);
    }
    let hot: f64 = mesh
        .points_m
        .iter()
        .zip(&r.reaction_w)
        .filter(|(p, _)| p[0] == 1.)
        .map(|(_, q)| q)
        .sum();
    close(hot, 200., 1e-6);
    let cold: f64 = mesh
        .points_m
        .iter()
        .zip(&r.reaction_w)
        .filter(|(p, _)| p[0] == 0.)
        .map(|(_, q)| q)
        .sum();
    close(cold, -200., 1e-6);
    close(r.balance_error_w, 0., 1e-6);
}

#[test]
fn quadratic_manufactured_solution_converges_under_refinement() {
    let exact = |p: [f64; 3]| 300. + p.iter().map(|x| x * (1. - x)).sum::<f64>();
    let mut errors = Vec::new();
    for n in [2, 4, 8] {
        let mut mesh = cube(n);
        for e in &mut mesh.elements {
            e.source_w_m3 = 12.;
        }
        let r = solve(&mesh, &boundary(&mesh, exact));
        let mut error = 0.;
        for e in &mesh.elements {
            let p = std::array::from_fn(|j| {
                e.nodes.iter().map(|i| mesh.points_m[*i][j]).sum::<f64>() / 4.
            });
            let t = e.nodes.iter().map(|i| r.temperature_k[*i]).sum::<f64>() / 4.;
            error += (t - exact(p)).powi(2);
        }
        errors.push((error / mesh.elements.len() as f64).sqrt());
        close(r.reaction_w.iter().sum(), -12., 1e-5);
        close(r.balance_error_w, 0., 1e-5);
    }
    println!("quadratic centroid RMS errors: {errors:?}");
    assert!(errors[1] < errors[0] * 0.3);
    assert!(errors[2] < errors[1] * 0.3);
}

#[test]
fn orientation_translation_and_length_scaling_preserve_temperature() {
    let base = cube(2);
    let bc = boundary(&base, |p| 300. + 50. * p[0]);
    let original = solve(&base, &bc);
    for scale in [0.001, 10.] {
        let mut mesh = base.clone();
        for p in &mut mesh.points_m {
            for x in p {
                *x = *x * scale + 2.;
            }
        }
        for e in &mut mesh.elements {
            e.nodes.swap(1, 2);
        }
        let r = solve(&mesh, &bc);
        for (a, b) in r.temperature_k.iter().zip(&original.temperature_k) {
            close(*a, *b, 1e-7);
        }
        for (a, b) in r.element_flux_w_m2.iter().zip(&original.element_flux_w_m2) {
            for j in 0..3 {
                close(a[j] * scale, b[j], 1e-6);
            }
        }
    }
}

#[test]
fn heterogeneous_conductivity_preserves_interface_flux() {
    let mut mesh = cube(4);
    for e in &mut mesh.elements {
        let x = e.nodes.iter().map(|i| mesh.points_m[*i][0]).sum::<f64>() / 4.;
        e.material = usize::from(x > 0.5);
    }
    let a = material(2.);
    let b = material(4.);
    let laws = [
        a.isotropic_conduction(MaterialPropertyFilter::default())
            .unwrap(),
        b.isotropic_conduction(MaterialPropertyFilter::default())
            .unwrap(),
    ];
    let bc: Vec<_> = mesh
        .points_m
        .iter()
        .enumerate()
        .filter(|(_, p)| p[0] == 0. || p[0] == 1.)
        .map(|(i, p)| (i, 300. + 100. * p[0]))
        .collect();
    let r = solve_steady(
        &mesh,
        &laws,
        &bc,
        &vec![0.; mesh.points_m.len()],
        SolveOptions::default(),
        || false,
    )
    .unwrap();
    let heat = 100. / (0.5 / 2. + 0.5 / 4.);
    for (p, t) in mesh.points_m.iter().zip(r.temperature_k) {
        let expected = 300. + heat * (p[0].min(0.5) / 2. + (p[0] - 0.5).max(0.) / 4.);
        close(t, expected, 1e-6);
    }
    for q in r.element_flux_w_m2 {
        close(q[0], -heat, 1e-5);
        close(q[1], 0., 1e-5);
        close(q[2], 0., 1e-5);
    }
}

#[test]
fn constraints_mesh_and_material_errors_never_return_a_result() {
    let mesh = cube(1);
    let m = material(2.);
    let laws = [m
        .isotropic_conduction(MaterialPropertyFilter::default())
        .unwrap()];
    let check = |mesh: &ThermalMesh, bc: &[(usize, f64)]| {
        solve_steady(
            mesh,
            &laws,
            bc,
            &vec![0.; mesh.points_m.len()],
            SolveOptions::default(),
            || false,
        )
    };
    assert!(check(&mesh, &[]).unwrap_err().contains("component"));
    assert!(check(&mesh, &[(0, 300.), (0, 300.)]).is_err());
    assert!(check(&mesh, &[(100, 300.)]).is_err());
    assert!(check(&mesh, &[(0, -1.)]).is_err());
    for variant in 0..6 {
        let mut bad = mesh.clone();
        match variant {
            0 => bad.elements[0].nodes[0] = usize::MAX,
            1 => bad.elements[0].nodes[0] = bad.elements[0].nodes[1],
            2 => bad.elements.push(bad.elements[0].clone()),
            3 => bad.elements[0].material = 1,
            4 => bad.points_m[0][0] = f64::NAN,
            _ => {
                for p in &mut bad.points_m {
                    p[2] = 0.;
                }
            }
        }
        assert!(check(&bad, &[(0, 300.)]).is_err(), "variant {variant}");
    }
    let mut disconnected = mesh.clone();
    let offset = disconnected.points_m.len();
    disconnected
        .points_m
        .extend(mesh.points_m.iter().map(|p| [p[0] + 2., p[1], p[2]]));
    disconnected
        .elements
        .extend(mesh.elements.iter().map(|e| ThermalTet {
            nodes: e.nodes.map(|i| i + offset),
            ..e.clone()
        }));
    assert!(check(&disconnected, &[(0, 300.)])
        .unwrap_err()
        .contains("component"));
}

#[test]
fn cancellation_and_work_limits_are_explicit_and_leave_inputs_unchanged() {
    let mesh = cube(3);
    let before = mesh.clone();
    let m = material(2.);
    let laws = [m
        .isotropic_conduction(MaterialPropertyFilter::default())
        .unwrap()];
    let bc = boundary(&mesh, |p| 300. + p[0]);
    let loads = vec![0.; mesh.points_m.len()];
    for limit in [0, mesh.elements.len() + 2] {
        let mut calls = 0;
        let error = solve_steady(&mesh, &laws, &bc, &loads, SolveOptions::default(), || {
            calls += 1;
            calls > limit
        })
        .unwrap_err();
        assert!(error.contains("cancelled"));
    }
    let options = SolveOptions {
        max_nodes: 1,
        ..Default::default()
    };
    assert!(solve_steady(&mesh, &laws, &bc, &loads, options, || false)
        .unwrap_err()
        .contains("budget"));
    let options = SolveOptions {
        max_iterations: 1,
        ..Default::default()
    };
    assert!(solve_steady(&mesh, &laws, &bc, &loads, options, || false)
        .unwrap_err()
        .contains("converge"));
    assert_eq!(mesh, before);
}

#[test]
fn integrated_nodal_power_is_balanced_by_boundary_reactions() {
    let mesh = cube(2);
    let m = material(2.);
    let laws = [m
        .isotropic_conduction(MaterialPropertyFilter::default())
        .unwrap()];
    let bc = boundary(&mesh, |_| 300.);
    let mut loads = vec![0.; mesh.points_m.len()];
    loads[13] = 6.;
    let result =
        solve_steady(&mesh, &laws, &bc, &loads, SolveOptions::default(), || false).unwrap();
    // Six unit-slope Cartesian conductances meet at the central node here.
    close(result.temperature_k[13], 301., 1e-8);
    close(result.reaction_w.iter().sum(), -6., 1e-8);
    close(result.balance_error_w, 0., 1e-8);
}

#[test]
fn invalid_loads_options_and_unphysical_solutions_fail() {
    let mesh = cube(2);
    let m = material(2.);
    let laws = [m
        .isotropic_conduction(MaterialPropertyFilter::default())
        .unwrap()];
    let bc = boundary(&mesh, |_| 0.);
    let mut loads = vec![0.; mesh.points_m.len()];
    loads[13] = f64::NAN;
    assert!(solve_steady(&mesh, &laws, &bc, &loads, SolveOptions::default(), || false).is_err());
    loads[13] = -1.;
    assert!(
        solve_steady(&mesh, &laws, &bc, &loads, SolveOptions::default(), || false)
            .unwrap_err()
            .contains("negative")
    );
    loads[13] = 0.;
    for tol in [0., -1., 1., f64::NAN, f64::INFINITY] {
        let options = SolveOptions {
            relative_tolerance: tol,
            ..Default::default()
        };
        assert!(solve_steady(&mesh, &laws, &bc, &loads, options, || false).is_err());
    }
    let zero = material(0.);
    assert!(solve_steady(
        &mesh,
        &[zero
            .isotropic_conduction(MaterialPropertyFilter::default())
            .unwrap()],
        &bc,
        &loads,
        SolveOptions::default(),
        || false
    )
    .unwrap_err()
    .contains("positive conductivity"));
}

#[test]
fn rigid_rotation_rotates_flux_without_changing_temperature() {
    let base = cube(2);
    let bc = boundary(&base, |p| 300. + 10. * p[0] - 20. * p[1] + 30. * p[2]);
    let original = solve(&base, &bc);
    let c = 0.7_f64.cos();
    let s = 0.7_f64.sin();
    let rotate = |p: [f64; 3]| [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]];
    let mut mesh = base.clone();
    for p in &mut mesh.points_m {
        *p = rotate(*p);
    }
    for e in &mut mesh.elements {
        e.nodes.rotate_left(1);
    }
    let result = solve(&mesh, &bc);
    for (a, b) in result.temperature_k.iter().zip(original.temperature_k) {
        close(*a, b, 1e-7);
    }
    for (actual, prior) in result
        .element_flux_w_m2
        .iter()
        .zip(original.element_flux_w_m2)
    {
        for (a, b) in actual.iter().zip(rotate(prior)) {
            close(*a, b, 1e-6);
        }
    }
}
