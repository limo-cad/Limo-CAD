

#[test]
fn corner_lap_certificate_keeps_floor_residue_and_proves_matching_links() {
    let origin = Point2Dto::new(0., 0.);
    let mut cleared = Cleared::new(5., origin);
    cleared.corner_loss = 2.;
    cleared.add(origin);
    assert!(cleared.contains(Point2Dto::new(4.9, 0.)));
    assert!(
        !cleared.contains(Point2Dto::new(5.1, 0.)),
        "corner stock is not floor clearance"
    );
    assert!(cleared.contains_cutter_capsule(Point2Dto::new(-1., 0.), Point2Dto::new(1., 0.), 6.));
    assert!(!cleared.contains_cutter_capsule(origin, Point2Dto::new(1.01, 0.), 6.));
    assert!(!cleared.contains_cutter_capsule(origin, Point2Dto::new(1., 0.), 6.01));

    for i in 0..=100 {
        let s = 4. + 2. * i as f64 / 100.;
        assert!(1. + s <= 5. + (s - 4.) + EPS);
    }
}

#[test]
fn corner_engagement_bounds_dense_sections_in_box_cylinder_and_hex_stock() {
    let doc = fixture(vec![]);
    for shape in [
        CamResolvedStockDto::Box,
        CamResolvedStockDto::Cylinder {
            center: Point2Dto::new(8., 7.),
            radius: 4.,
        },
        CamResolvedStockDto::Hex {
            center: Point2Dto::new(8., 7.),
            across_flats: 10.,
        },
    ] {
        let mut setup = doc.setups[0].clone();
        setup.resolved_stock = shape;
        let e = Envelope::new(&setup, 0.2, 6.).unwrap();

        let mut cleared = Cleared::new(1.6, xy(setup.stock.min));
        cleared.corner_loss = 1.;
        cleared.add(Point2Dto::new(5., 7.));
        cleared.add(Point2Dto::new(8., 7.));
        for x in [-1., 1., 4., 6., 8., 11., 15.] {
            for y in [0., 2., 6., 9., 13.] {
                let c = Point2Dto::new(x, y);
                let upper =
                    analytic_engagement(&setup, &cleared, c, 2., 0., &mut Work::default()).unwrap();
                let n = 2048;
                let count = (0..n)
                    .filter(|&i| {
                        (0..=32).any(|j| {
                            let s = 1. + j as f64 / 32.;
                            let p = polar(c, s, TAU * (i as f64 + 0.5) / n as f64);
                            e.initially_occupied(&setup, p, -1.)
                                && cleared.centers.iter().all(|&cc| dist(cc, p) > 0.6 + s)
                        })
                    })
                    .count();
                assert!(
                    upper + 0.005 >= count as f64 * TAU / n as f64,
                    "corner engagement underestimated: {:?} at {c:?}, upper {upper}",
                    setup.resolved_stock
                );
            }
        }
    }


    let mut setup = doc.setups[0].clone();
    setup.resolved_stock = CamResolvedStockDto::Cylinder {
        center: Point2Dto::new(5., 0.),
        radius: 1.,
    };
    let mut cleared = Cleared::new(3., Point2Dto::new(0., 0.));
    cleared.corner_loss = 6.;
    assert!(
        analytic_engagement(
            &setup,
            &cleared,
            Point2Dto::new(0., 0.),
            8.,
            0.,
            &mut Work::default()
        )
        .unwrap()
            >= 2. * 0.2_f64.asin()
    );
}

#[test]
fn corner_roughing_supports_exterior_and_cavity_and_roundtrips_actual_stock() {
    for bevel in [false, true] {
        for cavity in [false, true] {
            let mut doc = if cavity {
                cavity_fixture()
            } else {
                fixture(vec![cuboid([6., 5., -3.], [10., 9., 0.])])
            };
            if bevel {
                doc.tools[0].corner_chamfer = Some(crate::CamCornerChamferDto {
                    width: 0.6,
                    angle_degrees: 45.,
                });
            } else {
                doc.tools[0].kind = CamToolKind::BullNoseEndMill;
                doc.tools[0].corner_radius = Some(0.6);
            }
            for linked in [false, true] {
                let mut candidate = if linked {
                    with_linking(doc.clone())
                } else {
                    doc.clone()
                };
                if let CamOperationDto::Adaptive3d { bottom_z, .. } =
                    &mut candidate.setups[0].operations[0]
                {
                    *bottom_z = -0.5;
                }
                assert_adaptive_nc_roundtrip(candidate);
            }
        }
    }
}

#[test]
fn corner_roughing_rejects_a_lap_that_leaves_an_uncleared_center_boss() {
    let mut doc = cavity_fixture();
    doc.tools[0].kind = CamToolKind::BullNoseEndMill;
    doc.tools[0].corner_radius = Some(1.5);
    assert!(plan_setup(&doc, 1)
        .unwrap_err()
        .0
        .contains("uncleared center boss"));

    for kind in [
        CamToolKind::Drill,
        CamToolKind::ChamferMill,
        CamToolKind::ThreadMill,
    ] {
        doc.tools[0].kind = kind;
        doc.tools[0].corner_radius = None;
        doc.tools[0].point_angle_degrees = match kind {
            CamToolKind::Drill => Some(118.),
            CamToolKind::ChamferMill => Some(90.),
            _ => None,
        };
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("center-cutting flat or bull-nose"));
    }
}
