use super::*;

fn tool() -> CamToolDto {
    serde_json::from_value(serde_json::json!({
        "id":1,"number":1,"name":"Surface test cutter","kind":"flat_end_mill",
        "diameter":6.,"flute_length":12.,"overall_length":30.
    }))
    .unwrap()
}

#[test]
fn arc_bounds_cover_only_the_executed_sweep_including_wrap_and_full_circles() {
    for clockwise in [false, true] {
        for (start, end) in [(0.2_f64, 1.1_f64), (5.9, 0.3), (0.4, 0.4)] {
            let at = |a: f64| Point3Dto::new(5. * a.cos(), 5. * a.sin(), -2.);
            let arc = ArcSweep::new(
                at(start),
                Point3Dto::new(0., 0., -2.),
                at(end),
                clockwise,
                CamArcPlane::Xy,
            )
            .unwrap();
            let mut cuts = DisplayCuts::default();
            cuts.record_arc(&tool(), &arc, 0.002);
            let b = cuts.sweeps[0].center_bounds();
            for i in 0..=256 {
                let p = arc.point(i as f64 / 256.);
                for (axis, coordinate) in [p.x, p.y, p.z].into_iter().enumerate() {
                    assert!(coordinate >= b.min[axis] - 1e-9 && coordinate <= b.max[axis] + 1e-9);
                }
            }
            if !clockwise && start == 0.2 {
                assert!(
                    b.min[0] > 2. && b.min[1] > 0.9,
                    "not the complete parent circle"
                );
            }
        }
    }
}

#[test]
fn bounded_memo_replaces_entries_without_returning_collision_values() {
    let memo = Memo::new(16);
    for key in 0_u64..1024 {
        memo.insert(key, key * 7);
        assert_eq!(memo.get(key), Some(key * 7));
        for old in key.saturating_sub(20)..key {
            assert!(memo.get(old).is_none_or(|value| value == old * 7));
        }
    }
    assert_eq!(memo.slots.len(), 16);
    assert!(memo.get(0).is_none());
}

#[test]
fn grouped_planar_and_vertical_sweeps_match_the_complete_ungrouped_union() {
    let mut cuts = DisplayCuts::default();
    let mut tool = tool();
    for layer in 0..3 {
        tool.kind = if layer == 1 {
            CamToolKind::ChamferMill
        } else {
            CamToolKind::BullNoseEndMill
        };
        tool.point_angle_degrees = (layer == 1).then_some(90.);
        tool.corner_radius = (layer != 1).then_some(0.8);
        let z = -2. + layer as f64 * 0.6;
        let at = |i| {
            let angle = i as f64 / 24. * std::f64::consts::TAU;
            Point3Dto::new(4. * angle.cos(), 4. * angle.sin(), z)
        };
        for i in 0..24 {
            cuts.record(&tool, at(i), at(i + 1));
        }
    }
    tool.kind = CamToolKind::Drill;
    tool.corner_radius = None;
    tool.point_angle_degrees = Some(118.);
    for x in [-2., 2.] {
        cuts.record(&tool, Point3Dto::new(x, 0., 2.), Point3Dto::new(x, 0., -3.));
    }

    cuts.record(
        &tool,
        Point3Dto::new(0., -2., -2.),
        Point3Dto::new(2., 0., -3.),
    );
    let refiner = Refiner::new(&cuts, [0.3; 3]);
    for x in -16..=16 {
        for y in [-1.7, 0.1, 2.2] {
            for z in [-3.1, -2.8, -1.9, -1.3, -0.8, 0.1, 1.7] {
                let p = [x as f64 * 0.4, y, z];
                let distance = cuts
                    .sweeps
                    .iter()
                    .enumerate()
                    .map(|(i, s)| s.sample(p, i, None).distance)
                    .fold(f64::INFINITY, f64::min);
                if distance < refiner.band * 2. - 1e-8 {
                    let actual = refiner.sample(p).unwrap();
                    assert!(
                        (actual.distance + distance).abs() < 1e-8,
                        "full union at {p:?}"
                    );
                }
            }
        }
    }
    assert!(!refiner.exhausted());
    assert_eq!(refiner.nodes.len(), 2 * cuts.sweeps.len() - 1);
}

#[test]
fn bvh_matches_the_union_not_the_nearest_individual_surface() {
    let mut cuts = DisplayCuts::default();
    let mut tool = tool();
    for i in 0..18 {
        tool.corner_radius = if i % 2 == 0 { Some(0.8) } else { None };
        cuts.record(
            &tool,
            Point3Dto::new(i as f64 * 0.21, -3., -2. - i as f64 * 0.1),
            Point3Dto::new(i as f64 * 0.21, 7., -2. - i as f64 * 0.1),
        );
    }
    let refiner = Refiner::new(&cuts, [0.3; 3]);
    for x in -10..30 {
        for z in -22..6 {
            let p = [x as f64 * 0.25, 2.3, z as f64 * 0.2];
            let expected = cuts
                .sweeps
                .iter()
                .enumerate()
                .map(|(i, s)| s.sample(p, i, None).distance)
                .fold(f64::INFINITY, f64::min);
            if expected.abs() < refiner.band {
                let actual = refiner.sample(p).unwrap();
                assert!((actual.distance + expected).abs() < 1e-9, "union at {p:?}");
            }
        }
    }
    assert!(refiner.nodes.len() < cuts.sweeps.len() * 2);
    assert!(!refiner.exhausted());
}

#[test]
fn sloping_cutter_sweeps_match_dense_independent_pose_samples() {
    for corner in [None, Some(0.7), Some(2.0)] {
        let mut tool = tool();
        tool.corner_radius = corner;
        let mut cuts = DisplayCuts::default();
        cuts.record(
            &tool,
            Point3Dto::new(-2., -1., 1.),
            Point3Dto::new(4., 2., -3.),
        );
        let sweep = &cuts.sweeps[0];
        for p in [
            [-1., 2., -0.5],
            [4., 0., -2.8],
            [0., 0., -0.8],
            [5., 4., -3.5],
        ] {
            let dense = (0..=8192)
                .map(|i| {
                    let center = lerp(sweep.from, sweep.to, i as f64 / 8192.);
                    sweep
                        .profile
                        .surface_at(
                            (p[0] - center.x).hypot(p[1] - center.y),
                            p[2] - center.z,
                            0.,
                        )
                        .0
                })
                .fold(f64::INFINITY, f64::min);
            let actual = sweep.sample(p, 0, None).distance;
            assert!(
                (actual - dense).abs() < 0.001,
                "sloped profile {corner:?} at {p:?}: {actual} vs {dense}"
            );
        }
    }
}

#[test]
fn sweep_and_work_limits_never_present_an_incomplete_cut_union() {
    let tool = tool();
    let mut cuts = DisplayCuts::default();
    for i in 0..=MAX_SWEEPS {
        let p = Point3Dto::new(i as f64, 0., -1.);
        cuts.record(&tool, p, p);
    }
    assert!(cuts.limited);
    assert_eq!(cuts.sweeps.len(), MAX_SWEEPS);
    assert!(cuts.bytes() <= MAX_SWEEPS * std::mem::size_of::<CutterSweep>());
    let refiner = Refiner::new(&cuts, [0.3; 3]);
    assert!(refiner.project([0., 0., 0.]).is_none());
    assert!(!refiner.analytic_stock());
    let mut cuts = DisplayCuts::default();
    cuts.record(
        &tool,
        Point3Dto::new(0., 0., -1.),
        Point3Dto::new(2., 0., -1.),
    );
    let refiner = Refiner::new(&cuts, [0.3; 3]);
    refiner.evaluations.set(MAX_FIELD_EVALUATIONS);
    assert!(refiner.sample([0., 0., -1.]).is_none());
    assert!(refiner.exhausted());
}

#[test]
fn vertical_lead_display_chords_respect_the_model_relative_error_bound() {
    let tool = tool();
    let arc = ArcSweep::new(
        Point3Dto::new(0., 0., 3.),
        Point3Dto::new(0., 0., 0.),
        Point3Dto::new(3., 0., 0.),
        true,
        CamArcPlane::Xz,
    )
    .unwrap();
    let mut cuts = DisplayCuts::default();
    let tolerance = 0.003;
    cuts.record_arc(&tool, &arc, tolerance);
    assert!(!cuts.limited);
    for sweep in &cuts.sweeps {
        let middle = lerp(sweep.from, sweep.to, 0.5);
        assert!((middle.x.hypot(middle.z) - 3.).abs() <= tolerance);
    }
}

#[test]
fn known_starting_stock_clips_cutter_projection_and_keeps_bevel_creases() {
    let spec = GridSpec {
        min: Point3Dto::new(-6., -6., -8.),
        dimensions: [40, 40, 27],
        cell_size: [0.3, 0.3, 8. / 27.],
        edge: 0.3,
    };
    let mut cuts = DisplayCuts {
        initial: Some(StockBoundary::new(&spec, CamResolvedStockDto::Box)),
        ..Default::default()
    };
    let mut tool = tool();
    tool.kind = CamToolKind::ChamferMill;
    tool.point_angle_degrees = Some(90.);
    let p = Point3Dto::new(0., 0., -2.);
    cuts.record(&tool, p, p);
    let refiner = Refiner::new(&cuts, spec.cell_size);
    let q = refiner
        .project_in_cell([2.02, 0., 0.], spec.cell_size)
        .unwrap()
        .0;
    assert!(
        q[2].abs() < 1e-7,
        "the top rim stays on the initial top, {q:?}"
    );
    assert!(
        (q[0].hypot(q[1]) - 2.).abs() < 1e-6,
        "the crease also lies on the real cone, {q:?}"
    );
}

#[test]
fn peck_merging_requires_overlapping_tip_travel_not_just_overlapping_flutes() {
    let mut tool = tool();
    tool.kind = CamToolKind::Drill;
    tool.point_angle_degrees = Some(118.);
    let mut cuts = DisplayCuts::default();
    let a = Point3Dto::new(0., 0., -8.);
    let b = Point3Dto::new(0., 0., 3.);
    cuts.record(&tool, a, a);
    cuts.record(&tool, b, b);
    assert_eq!(cuts.sweeps.len(), 2);
    let p = [2.9, 0., 4.5];
    assert!(cuts
        .sweeps
        .iter()
        .all(|s| s.sample(p, 0, None).distance > 0.));
    cuts.record(&tool, b, Point3Dto::new(0., 0., 2.));
    assert_eq!(cuts.sweeps.len(), 2, "overlapping pecks should still merge");
}
