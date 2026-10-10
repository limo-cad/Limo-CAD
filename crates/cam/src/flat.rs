//! 3D flat-area finishing, after Fusion's "Flat" strategy.
//!
//! Every horizontal planar target area between Top and Bottom is finished at
//! its own Z (plus axial stock to leave) with contour-parallel offset passes.
//!
//! * A conservative max-height raster of the target classifies, per flat
//!   level, blocked cells (target above the flat) and flat cells (the target
//!   top lies on it).
//! * The level field `L = min(D - (R + radial), F + R / 2)`: `D` is the
//!   distance from the cutter axis to any target surface above the flat and
//!   `F` the signed distance into the flat. `L >= 0` keeps the whole cutter
//!   radial stock off every surface above the floor while it overhangs open
//!   flat edges by half a radius, so those edges are cut clean.
//! * `D` is exact (clipped target triangles) in a thin band around the wall
//!   pass and a conservative raster distance elsewhere: the pass beside a
//!   wall sits on the requested radial stock, never inside it. A tessellated
//!   convex surface (boss, fillet) bulges outside its chords, so those
//!   chords are measured as arcs of the curvature their bend implies.
//! * Passes are the iso-lines `L = k * stepover` traced by marching squares,
//!   run from the innermost outwards so the wall pass comes last. Traced
//!   loops keep the field's high side on their left, which is climb milling
//!   for an M3 spindle; conventional reverses them.

use super::{ensure_program_budget, require_flute_length, CamPlanError, ProgramBuilder, EPSILON};
use crate::model::{
    CamAdaptiveGeometryDto, CamOperationDto, CamSetupDto, CamToolDto, MillingDirection, Point2Dto,
    Point3Dto,
};
use crate::simulation::squared_distance_transform_1d;
use std::collections::HashMap;

const MAX_CELLS: usize = 2_250_000;
const MAX_TRIANGLES: usize = 200_000;
/// Target material this little above a flat does not block it (mesh noise).
const FLAT_EPS: f64 = 1.0e-3;
/// Cosine of the steepest triangle still counted as part of a flat.
const FLAT_NORMAL: f64 = 0.999_96;
/// Deviation allowed when thinning traced passes; added to the wall stock so
/// a thinned chord never moves toward a wall.
const SIMPLIFY: f64 = 2.0e-3;
/// Exact wall distances are evaluated this many cells either side of the
/// wall pass.
const EXACT_BAND_CELLS: f64 = 3.0;

pub(super) fn plan(
    builder: &mut ProgramBuilder,
    setup: &CamSetupDto,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    let CamOperationDto::Flat3d {
        name,
        top_z,
        bottom_z,
        parameters: p,
        geometry,
        cutting,
        ..
    } = operation
    else {
        unreachable!();
    };
    let geometry = geometry.as_ref().ok_or_else(|| {
        CamPlanError(format!(
            "flat finishing operation '{name}' has no captured target geometry; regenerate it"
        ))
    })?;
    let r = tool.diameter * 0.5;
    let triangles = setup_triangles(setup, geometry, name)?;
    let levels = flat_levels(&triangles, *bottom_z, *top_z, tool.diameter);
    if levels.is_empty() {
        return Err(CamPlanError(format!(
            "flat finishing operation '{name}' found no flat target areas between its Top and Bottom heights"
        )));
    }
    let part_top = triangles
        .iter()
        .flat_map(|t| t.iter().map(|v| v.z))
        .fold(f64::NEG_INFINITY, f64::max);
    let grid = Grid::new(setup, p.tolerance, r + 4.0 * p.tolerance.max(0.05))?;
    if grid.h > p.tolerance + EPSILON {
        builder.warnings.push(format!(
            "Flat '{name}' detects flats on a {:.3} mm grid (coarser than its {:.3} mm tolerance to stay within {MAX_CELLS} cells); wall passes still use exact target geometry.",
            grid.h, p.tolerance
        ));
    }
    let heights = grid.rasterize(&triangles);
    let curvatures = convex_curvatures(&triangles);
    let keep = p.radial_stock_to_leave + SIMPLIFY + grid.h * grid.h / (4.0 * r) + 1.0e-4;
    let retract_clears =
        builder.retract_z >= part_top + 1.0 && builder.retract_z >= builder.incoming_top + EPSILON;
    let land = r - tool.corner_radius.unwrap_or(0.0);
    let mut passes = 0usize;
    for &level in &levels {
        let depth = level + p.axial_stock_to_leave;
        let (field, walled, floor) =
            grid.level_field(&triangles, &curvatures, &heights, level, r, keep);
        let maximum = field.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if maximum < 0.0 {
            builder.warnings.push(format!(
                "Flat '{name}': the {:.3} mm cutter does not fit the flat at Z{level:.3}; it is left for a smaller tool.",
                tool.diameter
            ));
            continue;
        }
        require_flute_length(tool, part_top.max(builder.incoming_top) - depth, name)?;
        let rings = passes_for(&grid, &field, &walled, &floor, maximum, land, p.step_over);
        ensure_program_budget(
            builder.commands.len(),
            rings.iter().map(|ring| ring.len() + 8).sum(),
            name,
        )?;
        for mut ring in rings {
            if matches!(p.direction, MillingDirection::Conventional) {
                ring.reverse();
            }
            if let Some(q) = builder.position {
                let start = nearest_vertex(&ring, Point2Dto::new(q.x, q.y));
                ring.rotate_left(start);
            }
            let entry = ring[0];
            let stay = builder.position.is_some_and(|q| {
                (q.z - depth).abs() < EPSILON
                    && distance(Point2Dto::new(q.x, q.y), entry) <= p.stay_down_distance
                    && grid.segment_inside(&field, Point2Dto::new(q.x, q.y), entry)
            });
            if stay {
                builder.linear(Point3Dto::new(entry.x, entry.y, depth), cutting.feed_xy);
            } else {
                builder.require_clear_approach(entry, r, name)?;
                if retract_clears && builder.position.is_some() {
                    let q = builder.position.expect("checked above");
                    if q.z < builder.retract_z - EPSILON {
                        builder.rapid(Point3Dto::new(q.x, q.y, builder.retract_z));
                    }
                    let q = builder.position.expect("set above");
                    builder.rapid(Point3Dto::new(entry.x, entry.y, q.z));
                    builder.rapid(Point3Dto::new(entry.x, entry.y, builder.feed_plane(depth)));
                    builder.linear(Point3Dto::new(entry.x, entry.y, depth), cutting.feed_z);
                } else {
                    builder.approach(entry, depth, cutting.feed_z);
                }
            }
            for point in ring.iter().skip(1).chain(std::iter::once(&entry)) {
                builder.linear(Point3Dto::new(point.x, point.y, depth), cutting.feed_xy);
            }
            passes += 1;
        }
    }
    if passes == 0 {
        return Err(CamPlanError(format!(
            "flat finishing operation '{name}': the {:.3} mm cutter fits none of the detected flats",
            tool.diameter
        )));
    }
    builder.warnings.push(format!(
        "Flat '{name}': {passes} offset passes over {} flat level(s). Flat areas are taken from the target mesh; floors under overhangs and areas narrower than the cutter are not machined.",
        levels.len()
    ));
    builder.retract_to_clearance();
    Ok(())
}

/// The passes for one flat level, in cutting order.
///
/// Offset levels run from the walls (and open edges) inward, but only as far
/// as needed: the field is a distance, so the floor deeper than a pass's flat
/// land beyond the innermost level is already swept. Each pass is linked to
/// the pass one level deeper on its high side; a chain runs from the deepest
/// pass outward to its edge, so every pass has its already-cut side on the
/// left and the uncut side on the right (climb for M3). Chains enclosing
/// less area run first, finishing the inside of a ring of passes before the
/// outside. A pass that only re-sweeps floor other passes already cover is
/// dropped, unless it finishes a wall.
#[allow(clippy::too_many_arguments)]
fn passes_for(
    grid: &Grid,
    field: &[f64],
    walled: &[bool],
    floor: &[bool],
    maximum: f64,
    land: f64,
    step: f64,
) -> Vec<Vec<Point2Dto>> {
    let reach = land - 3.0 * grid.h - SIMPLIFY;
    let needed = if reach > 0.0 {
        ((maximum - reach) / step).ceil().max(0.0) as usize
    } else {
        (maximum / step).floor() as usize
    };
    let mut offsets: Vec<f64> = (0..=needed)
        .map(|k| (k as f64 * step).min(maximum - 2.0 * grid.h).max(0.0))
        .collect();
    offsets.dedup_by(|a, b| (*a - *b).abs() <= EPSILON);
    struct Ring {
        level: usize,
        points: Vec<Point2Dto>,
        area: f64,
        wall: bool,
        parent: Option<usize>,
        kept: bool,
    }
    let mut rings = Vec::new();
    for (k, &offset) in offsets.iter().enumerate() {
        for ring in grid.iso_loops(field, offset) {
            let points = simplify_closed(&ring, SIMPLIFY);
            if points.len() < 3 {
                continue;
            }
            let area = (0..points.len())
                .map(|i| {
                    let (a, b) = (points[i], points[(i + 1) % points.len()]);
                    a.x * b.y - b.x * a.y
                })
                .sum::<f64>()
                * 0.5;
            let wall = points.iter().any(|&q| grid.near(walled, q));
            rings.push(Ring {
                level: k,
                points,
                area,
                wall,
                parent: None,
                kept: true,
            });
        }
    }
    let polyline = |points: &[Point2Dto], q: Point2Dto| {
        (0..points.len())
            .map(|i| segment_distance(q, points[i], points[(i + 1) % points.len()]))
            .fold(f64::INFINITY, f64::min)
    };
    for i in 0..rings.len() {
        if rings[i].level == 0 {
            continue;
        }
        let probe = rings[i].points[0];
        rings[i].parent = (0..rings.len())
            .filter(|&j| rings[j].level + 1 == rings[i].level)
            .min_by(|&a, &b| {
                polyline(&rings[a].points, probe).total_cmp(&polyline(&rings[b].points, probe))
            });
    }
    if reach > 0.0 && rings.len() > 1 {
        let c = (reach / 8.0).max(grid.h);
        let (cx, cy) = (
            ((grid.nx as f64 * grid.h) / c).ceil() as usize,
            ((grid.ny as f64 * grid.h) / c).ceil() as usize,
        );
        let mut need = vec![false; cx * cy];
        for i in (0..floor.len()).filter(|&i| floor[i]) {
            let p = grid.center((i % grid.nx) as isize, (i / grid.nx) as isize);
            let (x, y) = (
                ((p.x - grid.min.x) / c) as usize,
                ((p.y - grid.min.y) / c) as usize,
            );
            if x < cx && y < cy {
                need[x + cx * y] = true;
            }
        }
        let inner = reach - c * std::f64::consts::FRAC_1_SQRT_2;
        let mut stamp = vec![usize::MAX; cx * cy];
        let covered: Vec<Vec<usize>> = rings
            .iter()
            .enumerate()
            .map(|(r, ring)| {
                let mut cells = Vec::new();
                for i in 0..ring.points.len() {
                    let (a, b) = (ring.points[i], ring.points[(i + 1) % ring.points.len()]);
                    let x0 = ((a.x.min(b.x) - inner - grid.min.x) / c).floor().max(0.0) as usize;
                    let x1 = (((a.x.max(b.x) + inner - grid.min.x) / c).floor() as usize)
                        .min(cx.saturating_sub(1));
                    let y0 = ((a.y.min(b.y) - inner - grid.min.y) / c).floor().max(0.0) as usize;
                    let y1 = (((a.y.max(b.y) + inner - grid.min.y) / c).floor() as usize)
                        .min(cy.saturating_sub(1));
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            let cell = x + cx * y;
                            if !need[cell] || stamp[cell] == r {
                                continue;
                            }
                            let q = Point2Dto::new(
                                grid.min.x + (x as f64 + 0.5) * c,
                                grid.min.y + (y as f64 + 0.5) * c,
                            );
                            if segment_distance(q, a, b) <= inner {
                                stamp[cell] = r;
                                cells.push(cell);
                            }
                        }
                    }
                }
                cells
            })
            .collect();
        let mut counts = vec![0u32; cx * cy];
        for cells in &covered {
            for &cell in cells {
                counts[cell] += 1;
            }
        }
        let mut candidates = (0..rings.len())
            .filter(|&i| !rings[i].wall)
            .collect::<Vec<_>>();
        candidates.sort_by_key(|&i| (rings[i].level, covered[i].len()));
        for i in candidates {
            if rings.iter().filter(|ring| ring.kept).count() > 1
                && covered[i].iter().all(|&cell| counts[cell] >= 2)
            {
                rings[i].kept = false;
                for &cell in &covered[i] {
                    counts[cell] -= 1;
                }
            }
        }
    }
    let kept_parent = |mut i: usize| loop {
        match rings[i].parent {
            Some(p) if rings[p].kept => return Some(p),
            Some(p) => i = p,
            None => return None,
        }
    };
    let mut children = vec![Vec::new(); rings.len()];
    let mut roots = Vec::new();
    for i in (0..rings.len()).filter(|&i| rings[i].kept) {
        match kept_parent(i) {
            Some(p) => children[p].push(i),
            None => roots.push(i),
        }
    }
    let by_area = |list: &mut Vec<usize>| {
        list.sort_by(|&a, &b| rings[a].area.abs().total_cmp(&rings[b].area.abs()))
    };
    by_area(&mut roots);
    for list in &mut children {
        by_area(list);
    }
    let mut order = Vec::new();
    let mut stack: Vec<(usize, bool)> = roots.iter().rev().map(|&i| (i, false)).collect();
    while let Some((i, expanded)) = stack.pop() {
        if expanded {
            order.push(i);
        } else {
            stack.push((i, true));
            stack.extend(children[i].iter().rev().map(|&c| (c, false)));
        }
    }
    order
        .into_iter()
        .map(|i| std::mem::take(&mut rings[i].points))
        .collect()
}

fn setup_triangles(
    setup: &CamSetupDto,
    geometry: &CamAdaptiveGeometryDto,
    name: &str,
) -> Result<Vec<[Point3Dto; 3]>, CamPlanError> {
    let mut triangles = Vec::new();
    for mesh in &geometry.targets {
        if !mesh.positions.len().is_multiple_of(3)
            || !mesh.indices.len().is_multiple_of(3)
            || mesh.positions.iter().any(|v| !v.is_finite())
            || mesh
                .indices
                .iter()
                .any(|&i| i as usize >= mesh.positions.len() / 3)
        {
            return Err(CamPlanError(format!(
                "flat finishing operation '{name}' needs finite indexed target triangle meshes"
            )));
        }
        let vertices = mesh
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|v| {
                let d = [
                    v[0] - setup.wcs.origin.x,
                    v[1] - setup.wcs.origin.y,
                    v[2] - setup.wcs.origin.z,
                ];
                let dot = |a: [f64; 3]| d[0] * a[0] + d[1] * a[1] + d[2] * a[2];
                Point3Dto::new(
                    dot(setup.wcs.x_axis),
                    dot(setup.wcs.y_axis),
                    dot(setup.wcs.z_axis),
                )
            })
            .collect::<Vec<_>>();
        for tri in mesh.indices.as_chunks::<3>().0 {
            triangles.push([
                vertices[tri[0] as usize],
                vertices[tri[1] as usize],
                vertices[tri[2] as usize],
            ]);
        }
        if triangles.len() > MAX_TRIANGLES {
            return Err(CamPlanError(format!(
                "flat finishing operation '{name}' target exceeds {MAX_TRIANGLES} triangles"
            )));
        }
    }
    Ok(triangles)
}

/// Z of every upward horizontal target area within [bottom, top], highest
/// first. Areas smaller than (0.05 D)^2 are noise, as in Fusion.
fn flat_levels(triangles: &[[Point3Dto; 3]], bottom: f64, top: f64, diameter: f64) -> Vec<f64> {
    let mut found: Vec<(f64, f64)> = Vec::new();
    for t in triangles {
        let u = [t[1].x - t[0].x, t[1].y - t[0].y, t[1].z - t[0].z];
        let v = [t[2].x - t[0].x, t[2].y - t[0].y, t[2].z - t[0].z];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length <= EPSILON || n[2] / length < FLAT_NORMAL {
            continue;
        }
        let z = (t[0].z + t[1].z + t[2].z) / 3.0;
        if z < bottom - FLAT_EPS || z > top + FLAT_EPS {
            continue;
        }
        match found
            .iter_mut()
            .find(|(level, _)| (level - z).abs() <= FLAT_EPS)
        {
            Some((_, area)) => *area += length * 0.5,
            None => found.push((z, length * 0.5)),
        }
    }
    let minimum = (0.05 * diameter).powi(2);
    let mut levels = found
        .into_iter()
        .filter(|&(_, area)| area >= minimum)
        .map(|(z, _)| z)
        .collect::<Vec<_>>();
    levels.sort_by(|a, b| b.total_cmp(a));
    levels
}

struct Grid {
    min: Point2Dto,
    h: f64,
    nx: usize,
    ny: usize,
}

impl Grid {
    fn new(setup: &CamSetupDto, tolerance: f64, margin: f64) -> Result<Self, CamPlanError> {
        let width = setup.stock.max.x - setup.stock.min.x + 2.0 * margin;
        let height = setup.stock.max.y - setup.stock.min.y + 2.0 * margin;
        if !(width > 0.0 && height > 0.0) {
            return Err(CamPlanError(
                "flat finishing needs a setup stock with area".into(),
            ));
        }
        let mut h = tolerance;
        while (width / h).ceil() * (height / h).ceil() > MAX_CELLS as f64 {
            h *= 1.25;
        }
        Ok(Self {
            min: Point2Dto::new(setup.stock.min.x - margin, setup.stock.min.y - margin),
            h,
            nx: (width / h).ceil() as usize,
            ny: (height / h).ceil() as usize,
        })
    }

    fn center(&self, x: isize, y: isize) -> Point2Dto {
        Point2Dto::new(
            self.min.x + (x as f64 + 0.5) * self.h,
            self.min.y + (y as f64 + 0.5) * self.h,
        )
    }

    /// Highest target Z touching each cell (conservative for blocking).
    fn rasterize(&self, triangles: &[[Point3Dto; 3]]) -> Vec<f64> {
        let mut heights = vec![f64::NEG_INFINITY; self.nx * self.ny];
        for t in triangles {
            let range = |values: [f64; 3], origin: f64, n: usize| {
                let lo = ((values.iter().copied().fold(f64::INFINITY, f64::min) - origin) / self.h)
                    .floor()
                    .max(0.0) as usize;
                let hi = ((values.iter().copied().fold(f64::NEG_INFINITY, f64::max) - origin)
                    / self.h)
                    .floor();
                (lo, hi.min(n as f64 - 1.0))
            };
            let (x0, x1) = range(t.map(|p| p.x), self.min.x, self.nx);
            let (y0, y1) = range(t.map(|p| p.y), self.min.y, self.ny);
            if x1 < x0 as f64 || y1 < y0 as f64 {
                continue;
            }
            for y in y0..=y1 as usize {
                for x in x0..=x1 as usize {
                    let mut polygon = t.to_vec();
                    for (axis, limit, greater) in [
                        (0, self.min.x + x as f64 * self.h, true),
                        (0, self.min.x + (x + 1) as f64 * self.h, false),
                        (1, self.min.y + y as f64 * self.h, true),
                        (1, self.min.y + (y + 1) as f64 * self.h, false),
                    ] {
                        polygon = clip(polygon, axis, limit, greater);
                    }
                    let cell = &mut heights[x + self.nx * y];
                    for v in polygon {
                        *cell = cell.max(v.z);
                    }
                }
            }
        }
        heights
    }

    /// Euclidean distance from every cell center to the nearest marked one.
    fn distance(&self, marked: impl Fn(usize) -> bool) -> Vec<f64> {
        let mut d = (0..self.nx * self.ny)
            .map(|i| if marked(i) { 0.0 } else { f64::INFINITY })
            .collect::<Vec<_>>();
        let mut input = vec![0.0; self.nx.max(self.ny)];
        let mut output = input.clone();
        for y in 0..self.ny {
            let row = y * self.nx..(y + 1) * self.nx;
            squared_distance_transform_1d(&d[row.clone()], self.h, &mut output[..self.nx]);
            d[row].copy_from_slice(&output[..self.nx]);
        }
        for x in 0..self.nx {
            for y in 0..self.ny {
                input[y] = d[x + self.nx * y];
            }
            squared_distance_transform_1d(&input[..self.ny], self.h, &mut output[..self.ny]);
            for y in 0..self.ny {
                d[x + self.nx * y] = output[y].sqrt();
            }
        }
        d
    }

    fn level_field(
        &self,
        triangles: &[[Point3Dto; 3]],
        curvatures: &[f64],
        heights: &[f64],
        level: f64,
        r: f64,
        keep: f64,
    ) -> (Vec<f64>, Vec<bool>, Vec<bool>) {
        let blocked = |i: usize| heights[i] > level + FLAT_EPS;
        let floor = self.floor_mask(heights, level, 4.0 * r);
        let flat = |i: usize| floor[i];
        let half = self.h * std::f64::consts::FRAC_1_SQRT_2;
        let walls = self.distance(blocked);
        let to_flat = self.distance(flat);
        let off_flat = self.distance(|i| !flat(i));
        let wall_pass = r + keep;
        let band = (
            wall_pass - EXACT_BAND_CELLS * self.h,
            wall_pass + EXACT_BAND_CELLS * self.h,
        );
        let exact = WallIndex::new(triangles, curvatures, level + FLAT_EPS, band.1);
        let bulge = exact.bulge;
        let (field, walled) = (0..self.nx * self.ny)
            .map(|i| {
                let point = self.center((i % self.nx) as isize, (i / self.nx) as isize);
                let approximate = walls[i] - half - bulge;
                let wall = if blocked(i) {
                    0.0
                } else if approximate + 2.0 * half + bulge >= band.0 && approximate <= band.1 {
                    exact.distance(point, band.1).max(approximate)
                } else {
                    approximate
                };
                let into_flat = if flat(i) {
                    off_flat[i] - self.h * 0.5
                } else {
                    self.h * 0.5 - to_flat[i]
                };
                let wall = wall - wall_pass;
                let open = into_flat + r * 0.5;
                (wall.min(open), wall <= open)
            })
            .unzip();
        (field, walled, floor)
    }

    /// The flat at `level` plus every lower area it encloses that is no wider
    /// than `widest` and has no target above the flat (holes, as Fusion
    /// machines over holes up to 2 D): at the floor Z the cutter only meets
    /// air or stock there, and stock left over a hole is cut away.
    fn floor_mask(&self, heights: &[f64], level: f64, widest: f64) -> Vec<bool> {
        let flat = heights
            .iter()
            .map(|&z| (z - level).abs() <= FLAT_EPS)
            .collect::<Vec<_>>();
        let mut floor = flat.clone();
        let mut seen = vec![false; heights.len()];
        for start in 0..heights.len() {
            if seen[start] || flat[start] || heights[start] > level + FLAT_EPS {
                continue;
            }
            seen[start] = true;
            let (mut queue, mut cells) = (vec![start], Vec::new());
            let (mut lo, mut hi) = ((usize::MAX, usize::MAX), (0usize, 0usize));
            let mut enclosed = true;
            while let Some(i) = queue.pop() {
                cells.push(i);
                let (x, y) = (i % self.nx, i / self.nx);
                (lo, hi) = ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y)));
                if x == 0 || y == 0 || x + 1 == self.nx || y + 1 == self.ny {
                    enclosed = false;
                    continue;
                }
                for j in [i - 1, i + 1, i - self.nx, i + self.nx] {
                    if heights[j] > level + FLAT_EPS {
                        enclosed = false;
                    } else if !flat[j] && !seen[j] {
                        seen[j] = true;
                        queue.push(j);
                    }
                }
            }
            let width = (hi.0 - lo.0 + 1).max(hi.1 - lo.1 + 1) as f64 * self.h;
            if enclosed && width <= widest {
                for i in cells {
                    floor[i] = true;
                }
            }
        }
        floor
    }

    /// Whether any field sample around `p` is marked.
    fn near(&self, mask: &[bool], p: Point2Dto) -> bool {
        let x = ((p.x - self.min.x) / self.h - 0.5).floor() as isize;
        let y = ((p.y - self.min.y) / self.h - 0.5).floor() as isize;
        [(0, 0), (1, 0), (0, 1), (1, 1)]
            .into_iter()
            .any(|(dx, dy)| {
                let (x, y) = (x + dx, y + dy);
                x >= 0
                    && y >= 0
                    && (x as usize) < self.nx
                    && (y as usize) < self.ny
                    && mask[x as usize + self.nx * y as usize]
            })
    }

    fn sample(&self, field: &[f64], x: isize, y: isize) -> f64 {
        if x < 0 || y < 0 || x >= self.nx as isize || y >= self.ny as isize {
            -1.0e9
        } else {
            field[x as usize + self.nx * y as usize]
        }
    }

    /// Whether a straight move at depth keeps the cutter in the machined
    /// region (field >= 0) all the way.
    fn segment_inside(&self, field: &[f64], a: Point2Dto, b: Point2Dto) -> bool {
        let steps = (distance(a, b) / (self.h * 0.5)).ceil().max(1.0) as usize;
        (0..=steps).all(|i| {
            let t = i as f64 / steps as f64;
            let p = Point2Dto::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let x = ((p.x - self.min.x) / self.h - 0.5).floor() as isize;
            let y = ((p.y - self.min.y) / self.h - 0.5).floor() as isize;
            [(0, 0), (1, 0), (0, 1), (1, 1)]
                .into_iter()
                .all(|(dx, dy)| self.sample(field, x + dx, y + dy) >= -SIMPLIFY)
        })
    }

    /// Closed iso-lines of `field` at `level` through the cell centers, each
    /// with the side above the level on its left. Out-of-grid samples are
    /// below every level, so every loop closes.
    fn iso_loops(&self, field: &[f64], level: f64) -> Vec<Vec<Point2Dto>> {
        type Key = (isize, isize, u8);
        let mut next: HashMap<Key, (Key, Point2Dto)> = HashMap::new();
        let crossing = |a: (isize, isize), b: (isize, isize)| {
            let (va, vb) = (self.sample(field, a.0, a.1), self.sample(field, b.0, b.1));
            let t = ((level - va) / (vb - va)).clamp(0.0, 1.0);
            let (pa, pb) = (self.center(a.0, a.1), self.center(b.0, b.1));
            Point2Dto::new(pa.x + (pb.x - pa.x) * t, pa.y + (pb.y - pa.y) * t)
        };
        for y in -1..self.ny as isize {
            for x in -1..self.nx as isize {
                let corners = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
                let keys: [Key; 4] = [(x, y, 0), (x + 1, y, 1), (x, y + 1, 0), (x, y, 1)];
                let inside = corners.map(|(cx, cy)| self.sample(field, cx, cy) >= level);
                if inside.iter().all(|&v| v) || inside.iter().all(|&v| !v) {
                    continue;
                }
                let mut events = Vec::new();
                for e in 0..4 {
                    let (a, b) = (e, (e + 1) % 4);
                    if inside[a] != inside[b] {
                        events.push((e, inside[a]));
                    }
                }
                let center_inside = corners
                    .iter()
                    .map(|&(cx, cy)| self.sample(field, cx, cy))
                    .sum::<f64>()
                    / 4.0
                    >= level;
                let saddle = events.len() == 4;
                for (k, &(edge, leaving)) in events.iter().enumerate() {
                    if !leaving {
                        continue;
                    }
                    let partner = if saddle && !center_inside {
                        events[(k + events.len() - 1) % events.len()].0
                    } else {
                        events[(k + 1) % events.len()].0
                    };
                    let point = |e: usize| crossing(corners[e], corners[(e + 1) % 4]);
                    next.insert(keys[edge], (keys[partner], point(partner)));
                }
            }
        }
        let mut loops = Vec::new();
        while let Some(&first) = next.keys().next() {
            let mut ring = Vec::new();
            let mut key = first;
            while let Some((to, point)) = next.remove(&key) {
                ring.push(point);
                key = to;
                if key == first {
                    break;
                }
            }
            if ring.len() >= 3 {
                loops.push(ring);
            }
        }
        loops
    }
}

/// A clipped facet above a flat, in plan, with the curvature of the convex
/// surface it approximates. A vertical facet projects to a chord whose true
/// arc bulges toward the facet's outward normal; any other facet's outline
/// edges bulge away from its interior. Arcs carry a 25% sagitta margin.
struct Facet {
    outline: Vec<Point2Dto>,
    curvature: f64,
    chord: Option<(Point2Dto, Point2Dto, Point2Dto)>,
}

impl Facet {
    fn sagitta(&self, a: Point2Dto, b: Point2Dto) -> f64 {
        1.25 * self.curvature * distance(a, b).powi(2) / 8.0
    }

    /// Distance from the cutter axis to the true surface this facet stands for.
    fn distance(&self, p: Point2Dto) -> f64 {
        if let Some((a, b, out)) = self.chord {
            return arc_distance(p, a, b, self.sagitta(a, b), out);
        }
        if polygon_distance(&self.outline, p) <= 0.0 {
            return 0.0;
        }
        let n = self.outline.len();
        let area = (0..n)
            .map(|i| {
                let (a, b) = (self.outline[i], self.outline[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>();
        (0..n)
            .map(|i| {
                let (a, b) = (self.outline[i], self.outline[(i + 1) % n]);
                let l = distance(a, b).max(1.0e-12);
                let d = Point2Dto::new((b.x - a.x) / l, (b.y - a.y) / l);
                let out = if area >= 0.0 {
                    Point2Dto::new(d.y, -d.x)
                } else {
                    Point2Dto::new(-d.y, d.x)
                };
                arc_distance(p, a, b, self.sagitta(a, b), out)
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// Largest distance the true surface may lie beyond the mesh outline.
    fn bulge(&self) -> f64 {
        let n = self.outline.len();
        (0..n)
            .map(|i| self.sagitta(self.outline[i], self.outline[(i + 1) % n]))
            .fold(0.0, f64::max)
    }
}

/// Distance from `p` to the circular arc through `a` and `b` whose midpoint
/// lies `sagitta` beyond the chord on the `out` side.
fn arc_distance(p: Point2Dto, a: Point2Dto, b: Point2Dto, sagitta: f64, out: Point2Dto) -> f64 {
    let half = distance(a, b) * 0.5;
    if sagitta <= 1.0e-9 || half <= 1.0e-9 {
        return segment_distance(p, a, b);
    }
    let radius = (half * half + sagitta * sagitta) / (2.0 * sagitta);
    let mid = Point2Dto::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
    let center = Point2Dto::new(
        mid.x - out.x * (radius - sagitta),
        mid.y - out.y * (radius - sagitta),
    );
    let v = Point2Dto::new(p.x - center.x, p.y - center.y);
    let length = v.x.hypot(v.y);
    if length > 1.0e-12 && (v.x * out.x + v.y * out.y) / length >= (radius - sagitta) / radius {
        (length - radius).abs()
    } else {
        distance(p, a).min(distance(p, b))
    }
}

/// Target surfaces above a flat, clipped to it and projected to XY, bucketed
/// for exact cutter-axis distance queries within `reach`.
struct WallIndex {
    facets: Vec<Facet>,
    buckets: HashMap<(i64, i64), Vec<usize>>,
    size: f64,
    /// Largest bulge of any facet beyond its mesh outline.
    bulge: f64,
}

impl WallIndex {
    fn new(triangles: &[[Point3Dto; 3]], curvatures: &[f64], above: f64, reach: f64) -> Self {
        let size = reach.max(1.0e-3);
        let mut facets = Vec::new();
        let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (t, &curvature) in triangles.iter().zip(curvatures) {
            if t.iter().all(|v| v.z <= above) {
                continue;
            }
            let outline = clip(t.to_vec(), 2, above, true)
                .into_iter()
                .map(|v| Point2Dto::new(v.x, v.y))
                .collect::<Vec<_>>();
            if outline.is_empty() {
                continue;
            }
            let u = [t[1].x - t[0].x, t[1].y - t[0].y, t[1].z - t[0].z];
            let v = [t[2].x - t[0].x, t[2].y - t[0].y, t[2].z - t[0].z];
            let n = Point2Dto::new(u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2]);
            let (mut a, mut b, mut far) = (outline[0], outline[0], 0.0);
            for &p in &outline {
                for &q in &outline {
                    if distance(p, q) > far {
                        (a, b, far) = (p, q, distance(p, q));
                    }
                }
            }
            let thin = far > EPSILON
                && outline.iter().all(|&q| segment_distance(q, a, b) <= 1.0e-3)
                && n.x.hypot(n.y) > EPSILON;
            let chord = thin.then(|| {
                let l = n.x.hypot(n.y);
                (a, b, Point2Dto::new(n.x / l, n.y / l))
            });
            let facet = Facet {
                outline,
                curvature,
                chord,
            };
            let (lo, hi) = facet.outline.iter().fold(
                (
                    Point2Dto::new(f64::INFINITY, f64::INFINITY),
                    Point2Dto::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
                ),
                |(lo, hi), p| {
                    (
                        Point2Dto::new(lo.x.min(p.x), lo.y.min(p.y)),
                        Point2Dto::new(hi.x.max(p.x), hi.y.max(p.y)),
                    )
                },
            );
            let reach = reach + facet.bulge();
            let index = facets.len();
            for bx in
                ((lo.x - reach) / size).floor() as i64..=((hi.x + reach) / size).floor() as i64
            {
                for by in
                    ((lo.y - reach) / size).floor() as i64..=((hi.y + reach) / size).floor() as i64
                {
                    buckets.entry((bx, by)).or_default().push(index);
                }
            }
            facets.push(facet);
        }
        let bulge = facets.iter().map(Facet::bulge).fold(0.0, f64::max);
        Self {
            facets,
            buckets,
            size,
            bulge,
        }
    }

    /// Exact distance to the nearest surface above the flat, or `cap` when
    /// none lies within it (every facet that close is in this bucket).
    fn distance(&self, p: Point2Dto, cap: f64) -> f64 {
        let key = (
            (p.x / self.size).floor() as i64,
            (p.y / self.size).floor() as i64,
        );
        self.buckets.get(&key).map_or(cap, |list| {
            list.iter()
                .map(|&i| self.facets[i].distance(p))
                .fold(cap, f64::min)
        })
    }
}

/// Curvature of the smooth convex surface each facet approximates. Where
/// the mesh turns convexly by a small angle phi across an edge (a
/// tessellated boss or fillet), a facet of width w spans an arc of
/// curvature phi / w; larger turns are real model edges.
fn convex_curvatures(triangles: &[[Point3Dto; 3]]) -> Vec<f64> {
    let normal = |t: &[Point3Dto; 3]| {
        let u = [t[1].x - t[0].x, t[1].y - t[0].y, t[1].z - t[0].z];
        let v = [t[2].x - t[0].x, t[2].y - t[0].y, t[2].z - t[0].z];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        (l > EPSILON).then(|| n.map(|c| c / l))
    };
    let key = |p: Point3Dto| {
        let q = |c: f64| (c * 1.0e5).round() as i64;
        (q(p.x), q(p.y), q(p.z))
    };
    let mut edges: HashMap<_, Vec<(usize, usize)>> = HashMap::new();
    for (i, t) in triangles.iter().enumerate() {
        for e in 0..3 {
            let (a, b) = (key(t[e]), key(t[(e + 1) % 3]));
            edges
                .entry(if a < b { (a, b) } else { (b, a) })
                .or_default()
                .push((i, e));
        }
    }
    let mut sags = vec![0.0; triangles.len()];
    for shared in edges.values().filter(|s| s.len() == 2) {
        let ((i, ei), (j, ej)) = (shared[0], shared[1]);
        let (Some(ni), Some(nj)) = (normal(&triangles[i]), normal(&triangles[j])) else {
            continue;
        };
        let phi = (ni[0] * nj[0] + ni[1] * nj[1] + ni[2] * nj[2])
            .clamp(-1.0, 1.0)
            .acos();
        if !(1.0e-6..=0.75).contains(&phi) {
            continue;
        }
        let centroid = |t: &[Point3Dto; 3]| {
            Point3Dto::new(
                (t[0].x + t[1].x + t[2].x) / 3.0,
                (t[0].y + t[1].y + t[2].y) / 3.0,
                (t[0].z + t[1].z + t[2].z) / 3.0,
            )
        };
        let (ci, cj) = (centroid(&triangles[i]), centroid(&triangles[j]));
        let toward = [cj.x - ci.x, cj.y - ci.y, cj.z - ci.z];
        if toward[0] * ni[0] + toward[1] * ni[1] + toward[2] * ni[2] >= 0.0 {
            continue;
        }
        for (k, e) in [(i, ei), (j, ej)] {
            let t = &triangles[k];
            let (a, b, c) = (t[e], t[(e + 1) % 3], t[(e + 2) % 3]);
            let ab = [b.x - a.x, b.y - a.y, b.z - a.z];
            let ac = [c.x - a.x, c.y - a.y, c.z - a.z];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let base = (ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2]).sqrt();
            if base <= EPSILON {
                continue;
            }
            let width =
                (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt() / base;
            sags[k] = f64::max(sags[k], phi / width);
        }
    }
    sags
}

fn polygon_distance(polygon: &[Point2Dto], p: Point2Dto) -> f64 {
    if polygon.len() >= 3 {
        let mut sign = 0.0f64;
        let mut inside = true;
        for i in 0..polygon.len() {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
            if cross.abs() <= 1.0e-12 {
                continue;
            }
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                inside = false;
                break;
            }
        }
        if inside && sign != 0.0 {
            return 0.0;
        }
    }
    (0..polygon.len())
        .map(|i| segment_distance(p, polygon[i], polygon[(i + 1) % polygon.len()]))
        .fold(f64::INFINITY, f64::min)
}

fn segment_distance(p: Point2Dto, a: Point2Dto, b: Point2Dto) -> f64 {
    let d = Point2Dto::new(b.x - a.x, b.y - a.y);
    let l2 = d.x * d.x + d.y * d.y;
    let t = if l2 <= 1.0e-18 {
        0.0
    } else {
        (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0)
    };
    distance(p, Point2Dto::new(a.x + d.x * t, a.y + d.y * t))
}

/// Sutherland-Hodgman clip of a convex polygon to one side of an axis plane.
fn clip(input: Vec<Point3Dto>, axis: usize, limit: f64, greater: bool) -> Vec<Point3Dto> {
    let coord = |p: Point3Dto| match axis {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    };
    let inside = |p| {
        if greater {
            coord(p) >= limit
        } else {
            coord(p) <= limit
        }
    };
    let mut out = Vec::new();
    let Some(&last) = input.last() else {
        return out;
    };
    let mut a = last;
    for b in input {
        if inside(a) != inside(b) {
            let t = ((limit - coord(a)) / (coord(b) - coord(a))).clamp(0.0, 1.0);
            out.push(Point3Dto::new(
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            ));
        }
        if inside(b) {
            out.push(b);
        }
        a = b;
    }
    out
}

fn distance(a: Point2Dto, b: Point2Dto) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn nearest_vertex(ring: &[Point2Dto], p: Point2Dto) -> usize {
    (0..ring.len())
        .min_by(|&a, &b| distance(ring[a], p).total_cmp(&distance(ring[b], p)))
        .unwrap_or(0)
}

/// Douglas-Peucker thinning of a closed loop, keeping its first vertex.
fn simplify_closed(ring: &[Point2Dto], tolerance: f64) -> Vec<Point2Dto> {
    if ring.len() < 4 {
        return ring.to_vec();
    }
    let far = nearest_vertex_far(ring);
    let mut out = simplify_open(&ring[..=far], tolerance);
    out.pop();
    let mut tail = ring[far..].to_vec();
    tail.push(ring[0]);
    let mut second = simplify_open(&tail, tolerance);
    second.pop();
    out.extend(second);
    out
}

fn nearest_vertex_far(ring: &[Point2Dto]) -> usize {
    (1..ring.len())
        .max_by(|&a, &b| distance(ring[a], ring[0]).total_cmp(&distance(ring[b], ring[0])))
        .unwrap_or(ring.len() / 2)
}

fn simplify_open(points: &[Point2Dto], tolerance: f64) -> Vec<Point2Dto> {
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (index, deviation) = (a + 1..b)
            .map(|i| (i, segment_distance(points[i], points[a], points[b])))
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .expect("non-empty span");
        if deviation > tolerance {
            keep[index] = true;
            stack.push((a, index));
            stack.push((index, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(&p, k)| k.then_some(p))
        .collect()
}
