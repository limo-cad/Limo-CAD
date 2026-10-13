fn detail_fixture(edge: f64) -> VoxelStock {
    let envelope = StockBoxDto {
        min: Point3Dto::new(-5., -5., -12.),
        max: Point3Dto::new(45., 29., 0.),
    };
    let spec = GridSpec::for_stock(&envelope, Some(edge), HARD_MAX_VOXELS).unwrap();
    let mut stock = VoxelStock::filled(&spec, |_| true);
    let mut tool = document().tools.remove(0);
    tool.flute_length = 20.;
    tool.kind = CamToolKind::BullNoseEndMill;
    tool.corner_radius = Some(0.8);
    let trace = |stock: &mut VoxelStock, tool: &CamToolDto, rectangle: [Point3Dto; 4]| {
        for i in 0..4 {
            stock
                .sweep_tool(
                    tool,
                    rectangle[i],
                    rectangle[(i + 1) % 4],
                    ToolSweepOptions {
                        mode: SweepMode::RemoveMaterial,
                        total_samples: &mut 0,
                        verification: None,
                        cancellation: None,
                    },
                )
                .unwrap();
        }
    };
    trace(
        &mut stock,
        &tool,
        [
            Point3Dto::new(-3., -3., -10.),
            Point3Dto::new(43., -3., -10.),
            Point3Dto::new(43., 27., -10.),
            Point3Dto::new(-3., 27., -10.),
        ],
    );
    tool.kind = CamToolKind::Drill;
    tool.diameter = 8.;
    tool.corner_radius = None;
    tool.point_angle_degrees = Some(118.);
    for x in [12., 28.] {
        stock
            .sweep_tool(
                &tool,
                Point3Dto::new(x, 12., 1.),
                Point3Dto::new(x, 12., -7.),
                ToolSweepOptions {
                    mode: SweepMode::RemoveMaterial,
                    total_samples: &mut 0,
                    verification: None,
                    cancellation: None,
                },
            )
            .unwrap();
    }
    tool.kind = CamToolKind::ChamferMill;
    tool.diameter = 6.;
    tool.point_angle_degrees = Some(90.);
    for x in [12., 28.] {
        let start = Point3Dto::new(x + 3.1, 12., -1.5);
        let arc = ArcSweep::new(
            start,
            Point3Dto::new(x, 12., -1.5),
            start,
            true,
            CamArcPlane::Xy,
        )
        .unwrap();
        stock
            .sweep_arc(&tool, &arc, SweepMode::RemoveMaterial, &mut 0, None, None)
            .unwrap();
    }
    trace(
        &mut stock,
        &tool,
        [
            Point3Dto::new(-0.9, -0.9, -1.5),
            Point3Dto::new(40.9, -0.9, -1.5),
            Point3Dto::new(40.9, 24.9, -1.5),
            Point3Dto::new(-0.9, 24.9, -1.5),
        ],
    );
    stock
}

#[test]
fn cutter_refined_chamfers_and_floor_fillets_follow_actual_sweeps() {
    let stock = detail_fixture(0.3);
    let words = stock.occupied.clone();
    let count = stock.occupied_count;
    let mesh = stock
        .presentation_mesh(MAX_SURFACE_TRIANGLES, &mut vec![])
        .unwrap();
    assert_eq!(stock.occupied, words);
    assert_eq!(stock.occupied_count, count);
    assert!(mesh.triangle_count < MAX_SURFACE_TRIANGLES);
    let mut chamfer = 0;
    let mut fillet = 0;
    let mut perimeter = 0;
    for (p, n) in mesh
        .positions
        .as_chunks::<3>().0.iter()
        .zip(mesh.normals.as_chunks::<3>().0.iter())
    {
        let [x, y, z] = [p[0] as f64, p[1] as f64, p[2] as f64];
        let r = (x - 12.).hypot(y - 12.);
        if z > -0.55 && z < -0.05 && r > 4.05 && r < 4.55 {
            assert!(
                (z - (r - 4.6)).abs() < 0.015,
                "hole bevel must follow the actual cone: {p:?}"
            );
            assert!(n[2] > 0.65 && n[2] < 0.78, "bevel normal: {n:?}");
            chamfer += 1;
        }
        if (5.0..35.0).contains(&x) && (-9.95..-9.25).contains(&z) && (-0.8..0.05).contains(&y) {


            assert!(
                ((y + 0.8).hypot(z + 9.2) - 0.8).abs() < 0.015,
                "floor fillet: {p:?}"
            );
            assert!(n[2] > 0.05 && n[1] < -0.05, "rounded normal: {n:?}");
            fillet += 1;
        }
        if (5.0..35.0).contains(&x) && (0.05..0.55).contains(&y) && (-0.55..-0.05).contains(&z) {
            assert!((z - (y - 0.6)).abs() < 0.015, "outer bevel: {p:?}");
            perimeter += 1;
        }
    }
    assert!(chamfer > 100, "missing small hole bevel ({chamfer})");


    for triangle in mesh.positions.as_chunks::<9>().0 {
        let p: [[f64; 3]; 3] =
            std::array::from_fn(|i| std::array::from_fn(|k| triangle[i * 3 + k] as f64));
        for i in 0..3 {
            let a = p[i];
            let b = p[(i + 1) % 3];
            if (a[0] - 20.) * (b[0] - 20.) >= 0. {
                continue;
            }
            let t = (20. - a[0]) / (b[0] - a[0]);
            let y = a[1] + (b[1] - a[1]) * t;
            let z = a[2] + (b[2] - a[2]) * t;
            if (-9.95..-9.25).contains(&z) && (-0.8..0.05).contains(&y) {
                assert!(
                    ((y + 0.8).hypot(z + 9.2) - 0.8).abs() < 0.015,
                    "fillet chord error at {y}, {z}"
                );
                fillet += 1;
            }
            if (0.05..0.55).contains(&y) && (-0.55..-0.05).contains(&z) {
                assert!(
                    (z - (y - 0.6)).abs() < 0.015,
                    "bevel cross-section at {y}, {z}"
                );
                perimeter += 1;
            }
        }
    }
    assert!(fillet > 10, "missing rounded floor ({fillet})");
    assert!(perimeter > 0, "missing perimeter bevel ({perimeter})");
    for (p, n) in mesh
        .positions
        .as_chunks::<9>().0.iter()
        .zip(mesh.normals.as_chunks::<9>().0.iter())
    {
        if [p[2], p[5], p[8]].iter().all(|z| z.abs() < 1e-6) {


            assert!(
                [n[2], n[5], n[8]].iter().all(|z| *z > 0.999),
                "top-face normals: {p:?}, {n:?}"
            );
        }
    }
}

#[test]
fn shallow_corner_display_retains_stock_instead_of_using_the_full_diameter() {
    let bounds = StockBoxDto {
        min: Point3Dto::new(-2., -6., -1.),
        max: Point3Dto::new(2., 6., 0.),
    };
    let spec = GridSpec::for_stock(&bounds, Some(0.1), HARD_MAX_VOXELS).unwrap();
    for bevel in [false, true] {
        let mut tool = document().tools.remove(0);
        tool.kind = CamToolKind::FlatEndMill;
        tool.diameter = 10.;
        tool.corner_radius = (!bevel).then_some(2.);
        tool.corner_chamfer = bevel.then_some(crate::CamCornerChamferDto {
            width: 2.,
            angle_degrees: 45.,
        });
        let mut stock = VoxelStock::filled(&spec, |_| true);
        stock
            .sweep_tool(
                &tool,
                Point3Dto::new(-10., 0., -0.5),
                Point3Dto::new(10., 0., -0.5),
                ToolSweepOptions {
                    mode: SweepMode::RemoveMaterial,
                    total_samples: &mut 0,
                    verification: None,
                    cancellation: None,
                },
            )
            .unwrap();
        let mesh = stock
            .presentation_mesh(MAX_SURFACE_TRIANGLES, &mut vec![])
            .unwrap();
        let mut tested = 0;


        for triangle in mesh.positions.as_chunks::<9>().0 {
            let points: [[f64; 3]; 3] =
                std::array::from_fn(|i| std::array::from_fn(|k| triangle[i * 3 + k] as f64));
            for i in 0..3 {
                let a = points[i];
                let b = points[(i + 1) % 3];
                if a[0] * b[0] >= 0. {
                    continue;
                }
                let t = -a[0] / (b[0] - a[0]);
                let y = (a[1] + (b[1] - a[1]) * t).abs();
                let z = a[2] + (b[2] - a[2]) * t;
                if (-0.48..-0.02).contains(&z) && (3.01..4.9).contains(&y) {
                    let expected = if bevel {
                        3. + z + 0.5
                    } else {
                        3. + (4. - (1.5 - z).powi(2)).sqrt()
                    };
                    assert!(
                        (y - expected).abs() < 0.012,
                        "shallow corner display, bevel={bevel}: Y={y}, Z={z} vs radius {expected}"
                    );
                    assert!(
                        y < 4.4,
                        "full-radius display would erase real shallow-cut residue"
                    );
                    tested += 1;
                }
            }
        }
        assert!(tested > 2, "missing shallow corner for bevel={bevel}");
    }
}

#[test]
fn incomplete_display_history_falls_back_to_the_entire_stock_surface() {
    let mut stock = detail_fixture(0.8);
    stock.display_cuts.limited = true;
    let mut warnings = vec![];
    let limited = stock
        .presentation_mesh(MAX_SURFACE_TRIANGLES, &mut warnings)
        .unwrap();
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("cutter-history limit")));
    stock.display_cuts = surface::DisplayCuts::default();
    let plain = stock
        .presentation_mesh(MAX_SURFACE_TRIANGLES, &mut vec![])
        .unwrap();
    assert_eq!(limited, plain);
    assert!(limited.triangle_count > 0);
}

#[test]
fn multi_operation_faced_stock_keeps_detailed_chamfers_at_the_default_work_budget() {
    let envelope = StockBoxDto {
        min: Point3Dto::new(-17., -10., 0.),
        max: Point3Dto::new(17., 10., 14.),
    };
    let spec = GridSpec::for_stock(&envelope, Some(0.115), HARD_MAX_VOXELS).unwrap();
    let mut stock = VoxelStock::filled(&spec, |_| true);
    let mut tool = document().tools.remove(0);
    let cut = |stock: &mut VoxelStock, tool: &CamToolDto, a, b| {
        stock
            .sweep_tool(
                tool,
                a,
                b,
                ToolSweepOptions {
                    mode: SweepMode::RemoveMaterial,
                    total_samples: &mut 0,
                    verification: None,
                    cancellation: None,
                },
            )
            .unwrap();
    };
    tool.kind = CamToolKind::FaceMill;
    tool.diameter = 50.;
    tool.corner_radius = Some(0.5);
    tool.flute_length = 5.;
    cut(
        &mut stock,
        &tool,
        Point3Dto::new(-44., 0., 12.),
        Point3Dto::new(44., 0., 12.),
    );
    tool.kind = CamToolKind::BullNoseEndMill;
    tool.diameter = 10.;
    tool.corner_radius = Some(1.);
    tool.flute_length = 25.;
    for (z, extra) in [(1.7, 0.25), (1., 0.)] {
        let loop_points = [
            Point3Dto::new(-19. - extra, -12. - extra, z),
            Point3Dto::new(19. + extra, -12. - extra, z),
            Point3Dto::new(19. + extra, 12. + extra, z),
            Point3Dto::new(-19. - extra, 12. + extra, z),
        ];
        for i in 0..4 {
            cut(&mut stock, &tool, loop_points[i], loop_points[(i + 1) % 4]);
        }
    }
    tool.kind = CamToolKind::Drill;
    tool.diameter = 5.5;
    tool.corner_radius = None;
    tool.point_angle_degrees = Some(118.);
    for x in [-8., 8.] {
        cut(
            &mut stock,
            &tool,
            Point3Dto::new(x, 0., 17.),
            Point3Dto::new(x, 0., 2.),
        );
    }
    tool.kind = CamToolKind::ChamferMill;
    tool.diameter = 6.;
    tool.point_angle_degrees = Some(90.);
    for x in [-8., 8.] {
        let at = |i| {
            let angle = i as f64 / 40. * std::f64::consts::TAU;
            Point3Dto::new(x + 1.75 * angle.cos(), 1.75 * angle.sin(), 10.5)
        };
        for i in 0..40 {
            cut(&mut stock, &tool, at(i), at(i + 1));
        }
    }
    let loop_points = [
        Point3Dto::new(-15., -8., 10.5),
        Point3Dto::new(15., -8., 10.5),
        Point3Dto::new(15., 8., 10.5),
        Point3Dto::new(-15., 8., 10.5),
    ];
    for i in 0..4 {
        cut(&mut stock, &tool, loop_points[i], loop_points[(i + 1) % 4]);
    }

    let mesh = stock
        .surface_mesh_with_refinement(MAX_SURFACE_TRIANGLES, true)
        .expect("detailed mesh must finish inside the unmodified work and triangle budgets");
    let mut chamfer_vertices = 0;
    for p in mesh.positions.as_chunks::<3>().0 {
        let r = (p[0] as f64 - 8.).hypot(p[1] as f64);
        if (2.85..3.1).contains(&r) && (11.6..11.9).contains(&(p[2] as f64)) {
            chamfer_vertices += 1;
        }
    }
    assert!(chamfer_vertices > 60);
    for triangle in mesh.positions.as_chunks::<9>().0 {
        let center: [f64; 3] =
            std::array::from_fn(|i| (triangle[i] + triangle[i + 3] + triangle[i + 6]) as f64 / 3.);
        if center[0].abs() < 13.
            && center[1].abs() < 6.
            && [-8., 8.]
                .into_iter()
                .all(|x| (center[0] - x).hypot(center[1]) > 3.4)
            && [triangle[2], triangle[5], triangle[8]]
                .into_iter()
                .any(|z| z > 11.99)
        {
            assert!(
                (center[2] - 12.).abs() < 1e-5,
                "a merged flat-face strip crossed into a chamfer: {triangle:?}"
            );
        }
    }
}

#[test]
#[ignore = "opt-in release-mode detailed-stock geometry/performance capture"]
fn capture_chamfers_and_fillets() {
    let start = std::time::Instant::now();
    let stock = detail_fixture(50. / 352.);
    let cut_time = start.elapsed();
    let begin_mesh = std::time::Instant::now();
    let mut warnings = vec![];
    let mesh = stock
        .presentation_mesh(MAX_SURFACE_TRIANGLES, &mut warnings)
        .unwrap();
    eprintln!("CAM detailed display: {:?} cells, {} triangles, cut {:.1} ms, mesh {:.1} ms, warnings {:?}", stock.dimensions, mesh.triangle_count, cut_time.as_secs_f64() * 1000., begin_mesh.elapsed().as_secs_f64() * 1000., warnings);
    if let Some(path) = std::env::var_os("LIMO_CAD_CAM_DETAIL_CAPTURE") {
        std::fs::write(path, serde_json::to_vec(&mesh).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "read-only capture of a locally supplied project; never a committed job fixture"]
fn capture_project_stock_detail() {
    let path = std::env::var("LIMO_CAD_CAM_DETAIL_PROJECT").expect("set LIMO_CAD_CAM_DETAIL_PROJECT");
    let contents = if path.ends_with(".limo") {
        let output = std::process::Command::new("unzip")
            .args(["-p", &path, "model.json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    } else {
        std::fs::read(path).unwrap()
    };
    let model: serde_json::Value = serde_json::from_slice(&contents).unwrap();
    let mut document: CamDocumentDto = serde_json::from_value(model["cam"].clone()).unwrap();


    for setup in &mut document.setups {
        setup.machine = None;
    }
    let setup = &document.setups[0];
    let spec = GridSpec::for_stock(&setup.stock, None, HARD_MAX_VOXELS).unwrap();
    let begin = std::time::Instant::now();
    let mut stock = initial_stock(&document, setup, &spec, None, None).unwrap();
    let program = plan_setup(&document, setup.id).unwrap();
    eprintln!(
        "Project stock: {:?}, {} program commands",
        spec.dimensions,
        program.commands.len()
    );
    run_program(
        &document,
        &program,
        &mut stock,
        ProgramRunOptions {
            collect: false,
            completed_steps: None,
            source_lines: &[],
            verification: None,
            checkpoints: None,
            cancellation: None,
            resume: None,
        },
    )
    .unwrap();
    eprintln!(
        "Cut {:.1} ms; display history bytes {}, limited {}",
        begin.elapsed().as_secs_f64() * 1000.,
        stock.display_cuts.bytes(),
        stock.display_cuts.limited
    );
    let begin = std::time::Instant::now();
    let mut warnings = vec![];
    let mesh = stock
        .presentation_mesh(MAX_SURFACE_TRIANGLES, &mut warnings)
        .unwrap();
    eprintln!(
        "Mesh {:.1} ms, {} triangles, warnings {:?}",
        begin.elapsed().as_secs_f64() * 1000.,
        mesh.triangle_count,
        warnings
    );
    if let Some(path) = std::env::var_os("LIMO_CAD_CAM_DETAIL_CAPTURE") {
        std::fs::write(path, serde_json::to_vec(&mesh).unwrap()).unwrap();
    }
}
