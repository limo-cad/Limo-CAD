#[test]
fn shallow_axial_cut_retains_the_analytic_round_or_beveled_corner_at_every_height() {
    let mut tool = document().tools.remove(0);
    tool.diameter = 10.;
    tool.flute_length = 10.;
    tool.overall_length = 30.;
    let bounds = StockBoxDto {
        min: Point3Dto::new(-2., -6., -1.),
        max: Point3Dto::new(2., 6., 0.),
    };
    let spec = GridSpec::for_stock(&bounds, Some(0.1), HARD_MAX_VOXELS).unwrap();
    let mut remaining = Vec::new();
    for shape in 0..4 {
        tool.kind = if shape == 1 {
            CamToolKind::BullNoseEndMill
        } else {
            CamToolKind::FlatEndMill
        };
        tool.corner_radius = matches!(shape, 1 | 2).then_some(2.);
        tool.corner_chamfer = (shape == 3).then_some(crate::CamCornerChamferDto {
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
        for z in 0..spec.dimensions[2] {
            for y in 0..spec.dimensions[1] {
                for x in 0..spec.dimensions[0] {
                    let p = stock.center(x, y, z);
                    let h = p.z + 0.5;
                    let radius = if h < 0. {
                        -1.
                    } else {
                        match shape {
                            1 | 2 => 3. + (4. - (2. - h).powi(2)).sqrt(),
                            3 => 3. + h,
                            _ => 5.,
                        }
                    };


                    if (p.y.abs() - radius).abs() > 0.01 {
                        assert_eq!(
                            stock.is_occupied_index(stock.index(x, y, z)),
                            p.y.abs() > radius,
                            "shape {shape} at {p:?}, expected cutting radius {radius}"
                        );
                    }
                }
            }
        }
        remaining.push(stock.occupied_count);
    }
    assert!(remaining[0] < remaining[1]);
    assert_eq!(
        remaining[1], remaining[2],
        "bull-nose and radiused flat end mill must agree"
    );
    assert!(
        remaining[2] < remaining[3],
        "a .5-deep bevel leaves more corner stock than the round"
    );
}
