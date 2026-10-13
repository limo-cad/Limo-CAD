#[test]
fn circular_roughing_is_continuous_with_full_retract_and_keep_down_off() {
    let center = Point2Dto::new(8.0, 7.0);
    let mut doc = with_linking(fixture(vec![cylinder(center, 2.5, -3.0, 0.0)]));
    doc.setups[0].stock_spec = CamStockSpecDto::FromModel {
        shape: CamStockShape::Cylinder,
        offsets: CamStockOffsetsDto::default(),
    };
    doc.setups[0].resolved_stock = CamResolvedStockDto::Cylinder {
        center,
        radius: 7.0,
    };
    doc.linking[0].keep_tool_down = false;
    doc.linking[0].maximum_stay_down = 0.01;
    doc.linking[0].retraction_policy = crate::CamRetractionPolicy::Full;
    for angle in [0.0_f64, 0.73] {
        let mut doc = doc.clone();
        let (sin, cos) = angle.sin_cos();
        let origin = Point3Dto::new(13.0, -8.0, 4.0);
        doc.setups[0].wcs.origin = origin;
        doc.setups[0].wcs.y_axis = [0.0, cos, sin];
        doc.setups[0].wcs.z_axis = [0.0, -sin, cos];
        let CamOperationDto::Adaptive3d {
            geometry: Some(geometry),
            ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        for mesh in &mut geometry.targets {
            for v in mesh.positions.as_chunks_mut::<3>().0 {
                let [x, y, z] = [v[0], v[1], v[2]];
                v.copy_from_slice(&[
                    origin.x + x,
                    origin.y + cos * y - sin * z,
                    origin.z + sin * y + cos * z,
                ]);
            }
        }
        for kind in [
            CamToolKind::FlatEndMill,
            CamToolKind::BullNoseEndMill,
            CamToolKind::FaceMill,
        ] {
            doc.tools[0].kind = kind;
            doc.tools[0].corner_radius = (kind != CamToolKind::FlatEndMill).then_some(0.4);
            doc.tools[0].maximum_axial_depth = (kind == CamToolKind::FaceMill).then_some(1.0);
            let program = plan_setup(&doc, 1).unwrap();
            assert!(
                program.stats.rapid_distance < 100.0,
                "{kind:?}: {:?}",
                program.stats
            );
            for z in [-1.0, -2.0] {
                let arcs: Vec<_> = program
                    .commands
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| match c {
                        CamCommandDto::Circular {
                            to,
                            feed,
                            clockwise: true,
                            ..
                        } if (to.z - z).abs() < EPS && (*feed - 600.0).abs() < EPS => Some(i),
                        _ => None,
                    })
                    .collect();
                assert!(
                    arcs.len() >= 8,
                    "must exercise multiple connected revolutions"
                );
                assert!(
                    program.commands[arcs[0]..=*arcs.last().unwrap()]
                        .iter()
                        .all(|c| match c {
                            CamCommandDto::Circular { to, feed, .. }
                            | CamCommandDto::Linear { to, feed } => {
                                (to.z - z).abs() < EPS && (*feed - 600.0).abs() < EPS
                            }
                            _ => false,
                        }),
                    "no exit, retract or separate entry inside the cutting pass"
                );
            }
            assert_adaptive_nc_roundtrip(doc.clone());
        }
    }
}

/// A cutting move of the pass: a clockwise arc about `center`, or a line.
#[derive(Clone, Copy)]
struct AuditedSpiralArc {
    from: Point2Dto,
    to: Point2Dto,
    center: Option<Point2Dto>,
}

#[test]
fn stock_cap_finishes_in_the_continuous_pass_without_cleanup_laps() {
    for kind in [
        CamToolKind::FlatEndMill,
        CamToolKind::BullNoseEndMill,
        CamToolKind::FaceMill,
    ] {
        let mut doc = with_linking(fixture(vec![cylinder(
            Point2Dto::new(8., 7.),
            3.,
            -3.,
            -1.,
        )]));
        doc.tools[0].kind = kind;
        doc.tools[0].corner_radius = (kind != CamToolKind::FlatEndMill).then_some(0.4);
        doc.tools[0].maximum_axial_depth = (kind == CamToolKind::FaceMill).then_some(1.0);
        let CamOperationDto::Adaptive3d { bottom_z, .. } = &mut doc.setups[0].operations[0] else {
            unreachable!()
        };
        *bottom_z = -0.9;
        let program = plan_setup(&doc, 1).unwrap();
        if kind != CamToolKind::FaceMill {
            assert!(
                program
                    .warnings
                    .iter()
                    .any(|w| w.contains("0 fallback rounded laps")),
                "{:?}",
                program.warnings
            );
        }
        assert!(program
            .warnings
            .iter()
            .any(|w| w.contains("0 helical entries")));
        assert!(point_is_cut_at_depth(
            &program,
            Point2Dto::new(8., 7.),
            -0.9,
            if kind == CamToolKind::FlatEndMill {
                2.
            } else {
                1.6
            }
        ));
        assert_adaptive_nc_roundtrip(doc);
    }
}
impl AuditedSpiralArc {
    fn radius(self, center: Point2Dto) -> f64 {
        dist(self.from, center)
    }
    /// Clockwise sweep of an arc, or the length of a line.
    fn sweep(self) -> f64 {
        match self.center {
            Some(c) => {
                let a = (self.from.y - c.y).atan2(self.from.x - c.x);
                let b = (self.to.y - c.y).atan2(self.to.x - c.x);
                let sweep = (a - b).rem_euclid(TAU);
                if sweep < 1e-9 { TAU } else { sweep }
            }
            None => dist(self.from, self.to),
        }
    }
    /// Position and travel heading at fraction t.
    fn at(self, t: f64) -> (Point2Dto, f64) {
        match self.center {
            Some(c) => {
                let a = (self.from.y - c.y).atan2(self.from.x - c.x) - self.sweep() * t;
                (polar(c, self.radius(c), a), a - PI / 2.)
            }
            None => (
                Point2Dto::new(
                    self.from.x + (self.to.x - self.from.x) * t,
                    self.from.y + (self.to.y - self.from.y) * t,
                ),
                (self.to.y - self.from.y).atan2(self.to.x - self.from.x),
            ),
        }
    }
    fn distance(self, p: Point2Dto) -> f64 {
        match self.center {
            Some(c) => {
                let a = (self.from.y - c.y).atan2(self.from.x - c.x);
                let t = (p.y - c.y).atan2(p.x - c.x);
                if (a - t).rem_euclid(TAU) <= self.sweep() + 1e-9 {
                    (dist(p, c) - self.radius(c)).abs()
                } else {
                    dist(p, self.from).min(dist(p, self.to))
                }
            }
            None => {
                let d = Point2Dto::new(self.to.x - self.from.x, self.to.y - self.from.y);
                let l2 = d.x * d.x + d.y * d.y;
                let t = (((p.x - self.from.x) * d.x + (p.y - self.from.y) * d.y) / l2).clamp(0., 1.);
                dist(p, Point2Dto::new(self.from.x + d.x * t, self.from.y + d.y * t))
            }
        }
    }
}

#[test]
fn spiral_sweeps_preserve_target_cover_stock_and_bound_section_engagement() {



    let doc = fixture(vec![]);
    let CamOperationDto::Adaptive3d { parameters, .. } = &doc.setups[0].operations[0] else {
        unreachable!()
    };
    for (floor, ae, protected, angle) in [
        (2.0, 1.0, 2.6, 0.0),
        (1.6, 0.5, 2.6, 0.7),
        (1.6, 4.0, 2.6, 2.4),
        (2.0, 1.0, 6.6, 0.4),
        (2.0, 1.0, 0.01, 1.3),
        (2.0, 1.0, 0.0, 0.3),
        (1.6, 0.5, -0.4, 0.8),
    ] {
        let mut p = parameters.clone();
        p.optimal_load = ae;
        let mut b = ProgramBuilder::new();
        b.clearance_z = 5.0;
        b.retract_z = 3.0;
        b.feed_height_z = 1.0;
        b.linking = Some(crate::CamLinkingDto {
            entry_positions: vec![polar(Point2Dto::new(0.0, 0.0), 20.0, angle)],
            ..Default::default()
        });
        let cap = protected <= floor - 2.0 + 1e-9;
        let footprint: Vec<_> = (0..128)
            .map(|i| polar(Point2Dto::new(0.0, 0.0), 7.0, TAU * i as f64 / 128.0))
            .collect();
        spiral::clear(&mut b,
        &footprint,
        (Point2Dto::new(0.0, 0.0), protected, cap),
        (2.0, floor, -1.0),
        &p,
        (600.0, 100.0),
        &mut Work::default())
        .unwrap();
        let mut arcs = vec![];
        let mut position = None;
        for command in &b.commands {
            match command {
                CamCommandDto::Circular {
                    to,
                    center,
                    feed,
                    clockwise,
                    ..
                } => {
                    if (*feed - 600.0).abs() < EPS {
                        assert!(*clockwise);
                        arcs.push(AuditedSpiralArc {
                            from: position.unwrap(),
                            to: Point2Dto::new(to.x, to.y),
                            center: Some(Point2Dto::new(center.x, center.y)),
                        });
                    }
                    position = Some(Point2Dto::new(to.x, to.y));
                }
                CamCommandDto::Linear { to, feed } => {
                    if (*feed - 600.0).abs() < EPS && !arcs.is_empty() {
                        arcs.push(AuditedSpiralArc {
                            from: position.unwrap(),
                            to: Point2Dto::new(to.x, to.y),
                            center: None,
                        });
                    }
                    position = Some(Point2Dto::new(to.x, to.y))
                }
                CamCommandDto::Rapid { to } => position = Some(Point2Dto::new(to.x, to.y)),
                _ => {}
            }
        }
        if arcs.last().is_some_and(|m| m.center.is_none()) {
            arcs.pop();
        }
        for pair in arcs.windows(2) {
            assert!(dist(pair[0].to, pair[1].from) < EPS);
            let (_, a) = pair[0].at(1.);
            let (_, b) = pair[1].at(0.);
            assert!((a - b).cos() > 1.0 - 1e-9, "C1 tangent join");
        }
        for s in [floor, (floor + 2.0) / 2.0, 2.0] {
            for (i, arc) in arcs.iter().enumerate() {
                for station in 0..=12 {
                    let (c, heading) = arc.at(station as f64 / 12.0);
                    assert!(
                        cap || dist(c, Point2Dto::new(0.0, 0.0)) - 2.0 >= protected - 1e-7,
                        "target clearance"
                    );
                    let mut contact = 0;
                    const N: usize = 360;
                    for k in 0..N {
                        let theta = heading - PI / 2. + (k as f64 + 0.5) * PI / N as f64;
                        let point = polar(c, s, theta);
                        if dist(point, Point2Dto::new(0.0, 0.0)) <= 7.0
                            && arcs[..i]
                                .iter()
                                .all(|prior| prior.distance(point) >= s - 1e-7)
                        {
                            contact += 1;
                        }
                    }
                    let measured = contact as f64 * PI / N as f64;
                    assert!(
                        measured <= (1.0 - ae / 2.0).acos() + 2.0 * PI / N as f64,
                        "section {s}, Ae {ae}, arc {i}, station {station}: {measured}"
                    );
                }
            }
            let residual = protected + 2.0 - s;
            for x in -30..=30 {
                for y in -30..=30 {
                    let point = Point2Dto::new(x as f64 * 7.0 / 30.0, y as f64 * 7.0 / 30.0);
                    let d = dist(point, Point2Dto::new(0.0, 0.0));
                    if d <= 7.0 && d > residual + 1e-7 {
                        assert!(
                            arcs.iter().any(|arc| arc.distance(point) <= s + 1e-7),
                            "uncut exterior sample {point:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn radial_lead_enters_perpendicular_from_air_within_the_engagement_limit() {
    let doc = fixture(vec![]);
    let CamOperationDto::Adaptive3d { parameters, .. } = &doc.setups[0].operations[0] else {
        unreachable!()
    };
    let (billet, r, floor, protected) = (7.0, 2.0, 2.0, 2.6);
    for (ae, angle) in [(1.0, 0.4), (0.5, 2.1), (1.6, 4.0)] {
        let mut p = parameters.clone();
        p.optimal_load = ae;
        let mut b = ProgramBuilder::new();
        b.clearance_z = 5.0;
        b.retract_z = 3.0;
        b.feed_height_z = 1.0;
        let mut link = crate::CamLinkingDto {
            entry_positions: vec![polar(Point2Dto::new(0.0, 0.0), 20.0, angle)],
            lead_in_feed: 450.0,
            ..Default::default()
        };
        link.lead_in.horizontal_radius = 1.0;
        link.lead_in.linear_distance = 0.0;
        b.linking = Some(link);
        let footprint: Vec<_> = (0..128)
            .map(|i| polar(Point2Dto::new(0.0, 0.0), billet, TAU * i as f64 / 128.0))
            .collect();
        spiral::clear(&mut b,
        &footprint,
        (Point2Dto::new(0.0, 0.0), protected, false),
        (r, floor, -1.0),
        &p,
        (600.0, 100.0),
        &mut Work::default())
        .unwrap();
        let u = Point2Dto::new(angle.cos(), angle.sin());
        let t = Point2Dto::new(u.y, -u.x);
        let xy = |p: &Point3Dto| Point2Dto::new(p.x, p.y);
        let lead = b
            .commands
            .iter()
            .position(|c| matches!(c, CamCommandDto::Circular { clockwise: false, .. }))
            .expect("counter-clockwise lead arc");
        let (CamCommandDto::Linear { to: arc_start, .. }, CamCommandDto::Circular { to: join, center: arc_center, .. }) =
            (&b.commands[lead - 1], &b.commands[lead])
        else {
            panic!("radial line then lead arc");
        };
        let plunge = b.commands[..lead - 1]
            .iter()
            .rev()
            .find_map(|c| match c {
                CamCommandDto::Linear { to, .. } | CamCommandDto::Rapid { to } if (to.z + 1.0).abs() < EPS => None,
                CamCommandDto::Linear { to, .. } | CamCommandDto::Rapid { to } => Some(xy(to)),
                _ => None,
            })
            .unwrap();
        let bottom = b.commands[..lead - 1]
            .iter()
            .rev()
            .find_map(|c| match c {
                CamCommandDto::Linear { to, .. } if (to.z + 1.0).abs() < EPS => Some(xy(to)),
                _ => None,
            })
            .unwrap_or(plunge);
        assert!(dist(bottom, Point2Dto::new(0.0, 0.0)) >= billet + r + 1.0 - 1e-6, "plunge clear of stock");
        let run = Point2Dto::new(arc_start.x - bottom.x, arc_start.y - bottom.y);
        let run_len = run.x.hypot(run.y);
        assert!(run_len < EPS || (run.x * -u.x + run.y * -u.y) / run_len > 1.0 - 1e-9, "radial approach");
        let after = &b.commands[lead + 1];
        let ring_start = match after {
            CamCommandDto::Linear { to, feed } => {
                assert!((*feed - 600.0).abs() < EPS);
                let d = Point2Dto::new(to.x - join.x, to.y - join.y);
                assert!((d.x * t.x + d.y * t.y) / d.x.hypot(d.y) > 1.0 - 1e-9, "straight run on the ring tangent");
                xy(to)
            }
            CamCommandDto::Circular { clockwise: true, .. } => xy(join),
            other => panic!("unexpected {other:?}"),
        };
        let v = Point2Dto::new(join.x - arc_center.x, join.y - arc_center.y);
        assert!(((-v.y) * t.x + v.x * t.y) / v.x.hypot(v.y) > 1.0 - 1e-9);
        let mut samples = vec![];
        for j in 0..=64 {
            let f = j as f64 / 64.0;
            samples.push((Point2Dto::new(bottom.x + run.x * f, bottom.y + run.y * f), Point2Dto::new(-u.x, -u.y)));
            samples.push((
                Point2Dto::new(join.x + (ring_start.x - join.x) * f, join.y + (ring_start.y - join.y) * f),
                t,
            ));
        }
        let from = (arc_start.y - arc_center.y).atan2(arc_start.x - arc_center.x);
        for j in 0..=64 {
            let a = from + PI * 0.5 * j as f64 / 64.0;
            samples.push((polar(xy(arc_center), 1.0, a), Point2Dto::new(-a.sin(), a.cos())));
        }
        for (c, heading) in samples {
            let heading = heading.y.atan2(heading.x);
            for s in [floor, r] {
                const N: usize = 720;
                let contact = (0..N)
                    .filter(|k| {
                        let theta = heading - PI / 2. + (*k as f64 + 0.5) * PI / N as f64;
                        dist(polar(c, s, theta), Point2Dto::new(0.0, 0.0)) <= billet
                    })
                    .count() as f64
                    * PI
                    / N as f64;
                assert!(contact <= (1.0 - ae / r).acos() + 2.0 * PI / N as f64, "Ae {ae}: {contact}");
            }
        }
    }
}
