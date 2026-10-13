//! Flat finishing on a stepped block: a square boss on a 40 x 30 block,
//! floors at Z0 (boss top) and Z-5 (the step), part bottom at Z-20.
use super::tests::{cutting, document, tool};
use super::{plan_setup, CamCommandDto};
use crate::model::{
    CamAdaptiveGeometryDto, CamFlatParametersDto, CamOperationDto, CamToolKind, MillingDirection,
    Point2Dto, Point3Dto,
};
use crate::simulation::CamStockMeshDto;

/// Closed box mesh with outward winding.
fn add_box(mesh: &mut CamStockMeshDto, min: [f64; 3], max: [f64; 3]) {
    let base = (mesh.positions.len() / 3) as u32;
    for &z in &[min[2], max[2]] {
        for &(x, y) in &[
            (min[0], min[1]),
            (max[0], min[1]),
            (max[0], max[1]),
            (min[0], max[1]),
        ] {
            mesh.positions.extend([x, y, z]);
        }
    }
    for f in [
        [0, 2, 1],
        [0, 3, 2], // bottom (down)
        [4, 5, 6],
        [4, 6, 7], // top (up)
        [0, 1, 5],
        [0, 5, 4], // -y
        [1, 2, 6],
        [1, 6, 5], // +x
        [2, 3, 7],
        [2, 7, 6], // +y
        [3, 0, 4],
        [3, 4, 7], // -x
    ] {
        mesh.indices.extend(f.map(|i| base + i));
    }
}

fn flat(stock_to_leave: f64, direction: MillingDirection) -> CamOperationDto {
    let mut mesh = CamStockMeshDto {
        positions: vec![],
        indices: vec![],
    };
    add_box(&mut mesh, [0.0, 0.0, -20.0], [40.0, 30.0, -5.0]);
    add_box(&mut mesh, [15.0, 10.0, -5.0], [25.0, 20.0, 0.0]);
    CamOperationDto::Flat3d {
        id: 1,
        name: "Flat".into(),
        enabled: true,
        tool_id: 2,
        top_z: 0.0,
        bottom_z: -20.0,
        clearance_z: 10.0,
        retract_z: 5.0,
        feed_height_z: 2.0,
        cutting: cutting(),
        parameters: CamFlatParametersDto {
            step_over: 2.0,
            radial_stock_to_leave: stock_to_leave,
            axial_stock_to_leave: 0.0,
            tolerance: 0.05,
            direction,
            stay_down_distance: 10.0,
        },
        geometry: Some(CamAdaptiveGeometryDto {
            targets: vec![mesh],
            stock: None,
        }),
    }
}

/// Feed moves at a Z, as segments.
fn cuts_at(commands: &[CamCommandDto], z: f64) -> Vec<(Point2Dto, Point2Dto)> {
    let mut at = None::<Point3Dto>;
    let mut out = vec![];
    for c in commands {
        match c {
            CamCommandDto::Linear { to, .. } => {
                if let Some(a) = at.filter(|a| (a.z - z).abs() < 1e-9 && (to.z - z).abs() < 1e-9) {
                    out.push((Point2Dto::new(a.x, a.y), Point2Dto::new(to.x, to.y)));
                }
                at = Some(*to);
            }
            CamCommandDto::Rapid { to } => at = Some(*to),
            _ => {}
        }
    }
    out
}

fn to_boss(p: Point2Dto) -> f64 {
    let dx = (15.0 - p.x).max(p.x - 25.0).max(0.0);
    let dy = (10.0 - p.y).max(p.y - 20.0).max(0.0);
    dx.hypot(dy)
}

fn segment_distance(p: Point2Dto, a: Point2Dto, b: Point2Dto) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0)
    };
    (p.x - a.x - dx * t).hypot(p.y - a.y - dy * t)
}

#[test]
fn flat_finishes_every_floor_at_its_own_z_and_keeps_radial_stock_off_walls() {
    for stock in [0.0, 0.3] {
        let doc = document(
            vec![flat(stock, MillingDirection::Climb)],
            vec![tool(2, CamToolKind::FlatEndMill, 6.0)],
        );
        let program = plan_setup(&doc, 1).unwrap();
        let feed_z = program
            .commands
            .iter()
            .filter_map(|c| match c {
                CamCommandDto::Linear { to, .. } => Some(to.z),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min);
        assert!((feed_z + 5.0).abs() < 1e-9, "deepest feed {feed_z}");
        let step = cuts_at(&program.commands, -5.0);
        let top = cuts_at(&program.commands, 0.0);
        assert!(!step.is_empty() && !top.is_empty());
        let closest = step
            .iter()
            .flat_map(|&(a, b)| {
                (0..=20).map(move |i| {
                    let t = i as f64 / 20.0;
                    Point2Dto::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
                })
            })
            .map(to_boss)
            .fold(f64::INFINITY, f64::min);
        assert!(
            closest >= 3.0 + stock - 1e-6,
            "stock {stock}: wall pass {closest}"
        );
        assert!(
            closest <= 3.0 + stock + 0.005,
            "stock {stock}: wall pass {closest}"
        );
        for i in 0..=80 {
            for j in 0..=60 {
                let p = Point2Dto::new(i as f64 * 0.5, j as f64 * 0.5);
                if to_boss(p) < 3.0 + stock + 0.01 + 0.5 {
                    continue; // corner and wall band the tool cannot reach
                }
                let reach = step
                    .iter()
                    .map(|&(a, b)| segment_distance(p, a, b))
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    reach <= 3.0 + 1e-6,
                    "step floor ({}, {}) uncovered: {reach}",
                    p.x,
                    p.y
                );
            }
        }
    }
}

#[test]
fn climb_runs_with_the_wall_on_the_right_and_conventional_reverses() {
    for (direction, wall_right) in [
        (MillingDirection::Climb, true),
        (MillingDirection::Conventional, false),
    ] {
        let doc = document(
            vec![flat(0.0, direction)],
            vec![tool(2, CamToolKind::FlatEndMill, 6.0)],
        );
        let program = plan_setup(&doc, 1).unwrap();
        for (a, b) in cuts_at(&program.commands, -5.0) {
            let mid = Point2Dto::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
            if to_boss(mid) > 3.0 + 0.01 || ((b.x - a.x).hypot(b.y - a.y)) < 0.5 {
                continue;
            }
            let center = Point2Dto::new(20.0, 15.0);
            let cross = (b.x - a.x) * (center.y - a.y) - (b.y - a.y) * (center.x - a.x);
            assert_eq!(cross < 0.0, wall_right, "{direction:?}");
        }
    }
}

#[test]
fn flat_rejects_flutes_that_cannot_reach_the_detected_floor() {
    let mut short = tool(2, CamToolKind::FlatEndMill, 6.0);
    short.flute_length = 1.0;
    let doc = document(vec![flat(0.0, MillingDirection::Climb)], vec![short]);
    let error = plan_setup(&doc, 1)
        .expect_err("a one-millimeter flute cannot finish the five-millimeter step");
    assert!(error.0.contains("flute length"), "{}", error.0);
}

#[test]
fn flat_flute_limit_uses_detected_floors_instead_of_the_unused_bottom_limit() {
    let mut exact = tool(2, CamToolKind::FlatEndMill, 6.0);
    exact.flute_length = 5.0;
    let doc = document(vec![flat(0.0, MillingDirection::Climb)], vec![exact]);
    assert!(plan_setup(&doc, 1).is_ok());
}

#[test]
fn flat_without_flats_in_range_is_an_error() {
    let mut op = flat(0.0, MillingDirection::Climb);
    if let CamOperationDto::Flat3d {
        top_z, bottom_z, ..
    } = &mut op
    {
        *top_z = -6.0;
        *bottom_z = -19.0;
    }
    let doc = document(vec![op], vec![tool(2, CamToolKind::FlatEndMill, 6.0)]);
    let error = plan_setup(&doc, 1).unwrap_err();
    assert!(
        error.0.contains("found no flat target areas"),
        "{}",
        error.0
    );
}

/// A round floor with a through hole: annulus r `hole`..15 at Z0 about (20, 15),
/// walls down to Z-5. Faces wind outward.
fn ring_floor(hole: f64) -> CamOperationDto {
    let (c, n) = (Point2Dto::new(20.0, 15.0), 96usize);
    let mut mesh = CamStockMeshDto {
        positions: vec![],
        indices: vec![],
    };
    for (radius, z) in [(15.0, 0.0), (hole, 0.0), (15.0, -5.0), (hole, -5.0)] {
        for i in 0..n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            mesh.positions
                .extend([c.x + radius * a.cos(), c.y + radius * a.sin(), z]);
        }
    }
    let v = |ring: usize, i: usize| (ring * n + i % n) as u32;
    for i in 0..n {
        mesh.indices.extend([
            v(0, i),
            v(1, i),
            v(1, i + 1),
            v(0, i),
            v(1, i + 1),
            v(0, i + 1),
        ]);
        mesh.indices.extend([
            v(2, i),
            v(3, i + 1),
            v(3, i),
            v(2, i),
            v(2, i + 1),
            v(3, i + 1),
        ]);
        mesh.indices.extend([
            v(0, i),
            v(0, i + 1),
            v(2, i + 1),
            v(0, i),
            v(2, i + 1),
            v(2, i),
        ]);
        mesh.indices.extend([
            v(1, i),
            v(3, i),
            v(3, i + 1),
            v(1, i),
            v(3, i + 1),
            v(1, i + 1),
        ]);
    }
    for tri in mesh.indices.as_chunks_mut::<3>().0 {
        tri.swap(1, 2);
    }
    let mut op = flat(0.0, MillingDirection::Climb);
    if let CamOperationDto::Flat3d { geometry, .. } = &mut op {
        *geometry = Some(CamAdaptiveGeometryDto {
            targets: vec![mesh],
            stock: None,
        });
    }
    op
}

#[test]
fn ring_floor_finishes_around_the_hole_first_with_every_pass_climbing() {
    let doc = document(
        vec![ring_floor(7.0)],
        vec![tool(2, CamToolKind::FlatEndMill, 6.0)],
    );
    let program = plan_setup(&doc, 1).unwrap();
    let c = Point2Dto::new(20.0, 15.0);
    let mut passes: Vec<Vec<Point2Dto>> = vec![];
    let mut current: Vec<Point2Dto> = vec![];
    for (a, b) in cuts_at(&program.commands, 0.0) {
        if current.is_empty() && !passes.is_empty() && passes.last().unwrap()[0] == a {
            current = vec![b];
            continue;
        }
        if current
            .last()
            .is_none_or(|&last| (last.x - a.x).hypot(last.y - a.y) > 1e-9)
        {
            current = vec![a];
        }
        current.push(b);
        if current.len() > 3 && (current[0].x - b.x).hypot(current[0].y - b.y) < 1e-9 {
            passes.push(std::mem::take(&mut current));
        }
    }
    assert!(passes.len() >= 2, "{} passes", passes.len());
    let mean = |pass: &[Point2Dto]| {
        pass.iter()
            .map(|p| (p.x - c.x).hypot(p.y - c.y))
            .sum::<f64>()
            / pass.len() as f64
    };
    let clockwise = |pass: &[Point2Dto]| {
        (0..pass.len() - 1)
            .map(|i| {
                (pass[i].x - c.x) * (pass[i + 1].y - c.y)
                    - (pass[i + 1].x - c.x) * (pass[i].y - c.y)
            })
            .sum::<f64>()
            < 0.0
    };
    let first_outer = passes
        .iter()
        .position(|p| !clockwise(p))
        .expect("outer passes");
    assert!(first_outer > 0, "starts on the inside");
    assert!(
        passes[first_outer..].iter().all(|p| !clockwise(p)),
        "back inside after leaving"
    );
    let radii: Vec<f64> = passes.iter().map(|p| mean(p)).collect();
    assert!(
        radii[..first_outer].windows(2).all(|w| w[1] < w[0]),
        "{radii:?}"
    );
    assert!(
        radii[first_outer..].windows(2).all(|w| w[1] > w[0]),
        "{radii:?}"
    );
    assert!(radii.iter().all(|&r| r > 7.0 && r < 15.0), "{radii:?}");
    let segments: Vec<(Point2Dto, Point2Dto)> = passes
        .iter()
        .flat_map(|p| p.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>())
        .collect();
    for i in 0..=80 {
        for k in 0..72 {
            let (radius, a) = (
                7.0 + 0.1 * i as f64,
                std::f64::consts::TAU * k as f64 / 72.0,
            );
            let q = Point2Dto::new(c.x + radius * a.cos(), c.y + radius * a.sin());
            let reach = segments
                .iter()
                .map(|&(a, b)| segment_distance(q, a, b))
                .fold(f64::INFINITY, f64::min);
            assert!(
                reach <= 3.0,
                "floor at r {radius:.1} uncovered ({reach:.3})"
            );
        }
    }
}

#[test]
fn a_hole_up_to_two_diameters_is_machined_over_so_no_stub_stays_on_it() {
    let doc = document(
        vec![ring_floor(4.0)],
        vec![tool(2, CamToolKind::FlatEndMill, 6.0)],
    );
    let program = plan_setup(&doc, 1).unwrap();
    let cuts = cuts_at(&program.commands, 0.0);
    let c = Point2Dto::new(20.0, 15.0);
    for i in 0..=40 {
        for k in 0..36 {
            let (radius, a) = (0.1 * i as f64, std::f64::consts::TAU * k as f64 / 36.0);
            let q = Point2Dto::new(c.x + radius * a.cos(), c.y + radius * a.sin());
            let reach = cuts
                .iter()
                .map(|&(a, b)| segment_distance(q, a, b))
                .fold(f64::INFINITY, f64::min);
            assert!(reach <= 3.0, "over the hole at r {radius:.1}: {reach:.3}");
        }
    }
}
