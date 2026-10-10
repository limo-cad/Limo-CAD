//! Original fixed-axis roughing with continuous convex exterior passes and
//! an engagement-limited circular-patch fallback for concavities/cavities.
//!
//! A conservative target upper envelope protects every height above a Z
//! level. Complete center-path circles sweep analytic disks because q is no
//! larger than the flat-land radius. Corner tools retain section-wise stock.
//! Those disks form the evolving stock used to test subsequent engagement
//! and links. This is a bounded roughing algorithm, not a finishing strategy.

use std::collections::{HashMap, HashSet, VecDeque};
use std::f64::consts::{PI, TAU};

use super::{depth_levels, ensure_program_budget, CamPlanError, ProgramBuilder};
use crate::model::{
    CamAdaptiveParametersDto, CamOperationDto, CamResolvedStockDto, CamSetupDto, CamToolDto,
    Point2Dto, Point3Dto,
};
use crate::simulation::{squared_distance_transform_1d, CamStockMeshDto};

#[path = "adaptive/exterior.rs"]
mod exterior;
#[path = "adaptive/face.rs"]
mod face;
#[path = "adaptive/layers.rs"]
mod layers;
#[path = "adaptive/linking.rs"]
mod linking;
#[path = "adaptive/spiral.rs"]
mod spiral;
use exterior::ConvexStock;

const MAX_CELLS: usize = 1_000_000;
const MAX_PATCHES: usize = 1_000_000;
const MAX_TRIANGLES: usize = 100_000;
const MAX_WORK: usize = 80_000_000;
const ANGLE_SAMPLES: usize = 128;
const LAP_SAMPLES: usize = 64;
const EPS: f64 = 1.0e-8;

#[derive(Default)]
struct Work(usize, [usize; 4]);
impl Work {
    fn spend(&mut self, count: usize, category: usize) -> Result<(), CamPlanError> {
        self.0 = self.0.saturating_add(count);
        self.1[category] = self.1[category].saturating_add(count);
        if self.0 > MAX_WORK {
            Err(CamPlanError(format!("High Speed Roughing exceeded its computation budget; use a larger optimal load/tolerance or split the depth range. Work counters (geometry/frontier/engagement/material): {:?}",self.1)))
        } else {
            Ok(())
        }
    }
}

struct Envelope {
    min: Point2Dto,
    h: f64,
    nx: usize,
    ny: usize,
    target: Vec<f64>,
    stock: Option<Vec<f64>>,
}

impl Envelope {
    fn new(setup: &CamSetupDto, h: f64, margin: f64) -> Result<Self, CamPlanError> {
        let nx = ((setup.stock.max.x - setup.stock.min.x + 2.0 * margin) / h).ceil() as usize;
        let ny = ((setup.stock.max.y - setup.stock.min.y + 2.0 * margin) / h).ceil() as usize;
        if nx == 0 || ny == 0 || nx.saturating_mul(ny) > MAX_CELLS {
            return Err(CamPlanError(format!("High Speed Roughing target grid exceeds {MAX_CELLS} cells; increase tolerance or reduce the setup stock bounds.")));
        }
        Ok(Self {
            min: Point2Dto::new(setup.stock.min.x - margin, setup.stock.min.y - margin),
            h,
            nx,
            ny,
            target: vec![f64::NEG_INFINITY; nx * ny],
            stock: None,
        })
    }

    fn index(&self, p: Point2Dto) -> Option<usize> {
        let x = ((p.x - self.min.x) / self.h).floor();
        let y = ((p.y - self.min.y) / self.h).floor();
        if x < 0.0 || y < 0.0 || x >= self.nx as f64 || y >= self.ny as f64 {
            None
        } else {
            Some(x as usize + self.nx * y as usize)
        }
    }

    fn center(&self, i: usize) -> Point2Dto {
        Point2Dto::new(
            self.min.x + (i % self.nx) as f64 * self.h + self.h * 0.5,
            self.min.y + (i / self.nx) as f64 * self.h + self.h * 0.5,
        )
    }

    fn rasterize(
        &self,
        mesh: &CamStockMeshDto,
        setup: &CamSetupDto,
        heights: &mut [f64],
        work: &mut Work,
    ) -> Result<(), CamPlanError> {
        if mesh.positions.is_empty()
            || !mesh.positions.len().is_multiple_of(3)
            || mesh.indices.is_empty()
            || !mesh.indices.len().is_multiple_of(3)
            || mesh.indices.len() / 3 > MAX_TRIANGLES
            || mesh.positions.len() / 3 > MAX_TRIANGLES * 3
            || mesh.positions.iter().any(|v| !v.is_finite())
            || mesh
                .indices
                .iter()
                .any(|&i| i as usize >= mesh.positions.len() / 3)
        {
            return Err(CamPlanError("High Speed Roughing geometry requires finite indexed triangle meshes within the triangle budget.".into()));
        }
        let vertices = mesh
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|v| {
                let r = [
                    v[0] - setup.wcs.origin.x,
                    v[1] - setup.wcs.origin.y,
                    v[2] - setup.wcs.origin.z,
                ];
                let dot = |axis: [f64; 3]| r[0] * axis[0] + r[1] * axis[1] + r[2] * axis[2];
                Point3Dto::new(
                    dot(setup.wcs.x_axis),
                    dot(setup.wcs.y_axis),
                    dot(setup.wcs.z_axis),
                )
            })
            .collect::<Vec<_>>();
        for tri in mesh.indices.as_chunks::<3>().0 {
            let t = [
                vertices[tri[0] as usize],
                vertices[tri[1] as usize],
                vertices[tri[2] as usize],
            ];
            let bounds = |x_axis: bool| {
                let values = t.map(|p| if x_axis { p.x } else { p.y });
                let origin = if x_axis { self.min.x } else { self.min.y };
                let n = if x_axis { self.nx } else { self.ny };
                let lo = ((values.into_iter().fold(f64::INFINITY, f64::min) - origin) / self.h)
                    .floor() as isize;
                let hi = ((values.into_iter().fold(f64::NEG_INFINITY, f64::max) - origin) / self.h)
                    .floor() as isize;
                (lo.max(0) as usize, hi.min(n as isize - 1))
            };
            let (x0, x1) = bounds(true);
            let (y0, y1) = bounds(false);
            if x1 < x0 as isize || y1 < y0 as isize {
                continue;
            }
            work.spend(
                (x1 as usize - x0 + 1).saturating_mul(y1 as usize - y0 + 1),
                0,
            )?;
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
                    for p in polygon {
                        heights[x + self.nx * y] = heights[x + self.nx * y].max(p.z);
                    }
                }
            }
        }
        Ok(())
    }

    fn clearance(&self, depth: f64, axial: f64) -> Vec<f64> {
        let mut d = self
            .target
            .iter()
            .map(|&z| {
                if z + axial > depth + EPS {
                    0.0
                } else {
                    f64::INFINITY
                }
            })
            .collect::<Vec<_>>();
        let mut input = vec![0.0; self.nx.max(self.ny)];
        let mut output = input.clone();
        for y in 0..self.ny {
            let start = y * self.nx;
            squared_distance_transform_1d(
                &d[start..start + self.nx],
                self.h,
                &mut output[..self.nx],
            );
            d[start..start + self.nx].copy_from_slice(&output[..self.nx]);
        }
        for x in 0..self.nx {
            for y in 0..self.ny {
                input[y] = d[x + self.nx * y];
            }
            squared_distance_transform_1d(&input[..self.ny], self.h, &mut output[..self.ny]);
            for y in 0..self.ny {
                d[x + self.nx * y] = output[y];
            }
        }
        d
    }

    fn initially_occupied(&self, setup: &CamSetupDto, p: Point2Dto, depth: f64) -> bool {
        if p.x < setup.stock.min.x
            || p.x > setup.stock.max.x
            || p.y < setup.stock.min.y
            || p.y > setup.stock.max.y
            || depth >= setup.stock.max.z - EPS
        {
            return false;
        }
        let below_stock_top = || {
            self.index(p)
                .is_some_and(|i| self.stock.as_ref().is_some_and(|s| s[i] > depth + EPS))
        };
        let profile = match &setup.resolved_stock {
            CamResolvedStockDto::Box => true,
            CamResolvedStockDto::Cylinder { center, radius } => dist(p, *center) <= *radius,
            CamResolvedStockDto::Hex {
                center,
                across_flats,
            } => {
                let x = (p.x - center.x).abs();
                let y = (p.y - center.y).abs();
                x <= across_flats * 0.5 && x + 3.0_f64.sqrt() * y <= *across_flats
            }
            CamResolvedStockDto::ModelBody { .. } | CamResolvedStockDto::Rest { .. } => {
                return below_stock_top()
            }
        };
        profile && (self.stock.is_none() || below_stock_top())
    }
}

/// Land on accessible horizontal terraces, then fill each interval with cuts
/// no deeper than maximum stepdown. Adding terraces to a separate global
/// schedule creates redundant nearby layers after a shoulder has reset Ap.
fn roughing_depth_levels(
    setup: &CamSetupDto,
    meshes: &[CamStockMeshDto],
    top: f64,
    bottom: f64,
    ceiling: f64,
    p: &CamAdaptiveParametersDto,
) -> Result<Vec<f64>, CamPlanError> {
    let terraces = roughing_terraces(setup, meshes, top, bottom, ceiling, p);
    let mut depths = Vec::new();
    let mut previous = top;
    for terrace in terraces {
        depths.extend(depth_levels(previous, terrace, p.maximum_stepdown)?);
        if depths.len() > 512 {
            return Err(CamPlanError(
                "High Speed Roughing is limited to 512 depth levels per operation.".into(),
            ));
        }
        previous = terrace;
    }
    Ok(depths)
}

/// Model shelves between `bottom` and `top` as cut levels. None above
/// `ceiling` (the selected Top plus axial allowance): material above Top is
/// incoming stock to respect, not geometry this operation was asked to cut.
fn roughing_terraces(
    setup: &CamSetupDto,
    meshes: &[CamStockMeshDto],
    top: f64,
    bottom: f64,
    ceiling: f64,
    p: &CamAdaptiveParametersDto,
) -> Vec<f64> {
    let mut terraces = vec![bottom];
    for mesh in meshes {
        let z = |i: u32| {
            let v = &mesh.positions[i as usize * 3..];
            (v[0] - setup.wcs.origin.x) * setup.wcs.z_axis[0]
                + (v[1] - setup.wcs.origin.y) * setup.wcs.z_axis[1]
                + (v[2] - setup.wcs.origin.z) * setup.wcs.z_axis[2]
        };
        for tri in mesh.indices.as_chunks::<3>().0 {
            let zs = [z(tri[0]), z(tri[1]), z(tri[2])];
            let high = zs.into_iter().fold(f64::NEG_INFINITY, f64::max);
            let low = zs.into_iter().fold(f64::INFINITY, f64::min);
            let level = high + p.axial_stock_to_leave;

            let v = [tri[0], tri[1], tri[2]].map(|i| &mesh.positions[i as usize * 3..][..3]);
            let u = [v[1][0] - v[0][0], v[1][1] - v[0][1], v[1][2] - v[0][2]];
            let w = [v[2][0] - v[0][0], v[2][1] - v[0][1], v[2][2] - v[0][2]];
            let normal = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            let up = normal
                .iter()
                .zip(setup.wcs.z_axis)
                .map(|(a, b)| a * b)
                .sum::<f64>();
            if up > EPS
                && high - low <= EPS
                && level < top - EPS
                && level <= ceiling + EPS
                && level > bottom + EPS
                && level < setup.stock.max.z - EPS
            {
                terraces.push(level);
            }
        }
    }
    terraces.sort_by(|a, b| b.total_cmp(a));
    terraces.dedup_by(|a, b| (*a - *b).abs() <= EPS);
    terraces
}

fn clip(input: Vec<Point3Dto>, axis: usize, limit: f64, greater: bool) -> Vec<Point3Dto> {
    let coord = |p: Point3Dto| if axis == 0 { p.x } else { p.y };
    let inside = |p| {
        if greater {
            coord(p) >= limit - EPS
        } else {
            coord(p) <= limit + EPS
        }
    };
    let mut out = Vec::new();
    if input.is_empty() {
        return out;
    }
    let mut a = *input.last().unwrap();
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

fn dist(a: Point2Dto, b: Point2Dto) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}
fn polar(c: Point2Dto, r: f64, theta: f64) -> Point2Dto {
    Point2Dto::new(c.x + r * theta.cos(), c.y + r * theta.sin())
}
fn xy(p: Point3Dto) -> Point2Dto {
    Point2Dto::new(p.x, p.y)
}

/// Analytic union of fully cut circular patches. Spatial buckets bound each
/// point query to nearby disks, and avoid using display/simulation pixels to
/// decide whether a stay-down move is safe.
struct Cleared {
    radius: f64,

    corner_loss: f64,
    origin: Point2Dto,
    centers: Vec<Point2Dto>,
    buckets: HashMap<(i32, i32), Vec<usize>>,
    /// Cells whose entire square is certified inside a removed disk. This
    /// is an exact broad phase, not a voxel approximation of stock removal.
    covered_tiles: HashSet<(i32, i32)>,
    /// After continuous exterior loops, all remaining stock lies inside
    /// this conservative rounded convex bound. It is a removal certificate,
    /// not a replacement for target geometry or the independent simulator.
    exterior: Option<ConvexStock>,
}
impl Cleared {
    fn new(radius: f64, origin: Point2Dto) -> Self {
        Self {
            radius,
            corner_loss: 0.0,
            origin,
            centers: Vec::new(),
            buckets: HashMap::new(),
            covered_tiles: HashSet::new(),
            exterior: None,
        }
    }
    fn key(&self, p: Point2Dto) -> (i32, i32) {
        (
            ((p.x - self.origin.x) / self.radius).floor() as i32,
            ((p.y - self.origin.y) / self.radius).floor() as i32,
        )
    }
    fn add(&mut self, p: Point2Dto) {
        self.buckets
            .entry(self.key(p))
            .or_default()
            .push(self.centers.len());
        self.centers.push(p);
        let h = self.radius * 0.25;
        let local = Point2Dto::new(p.x - self.origin.x, p.y - self.origin.y);
        let (tx, ty) = ((local.x / h).floor() as i32, (local.y / h).floor() as i32);
        for y in ty - 4..=ty + 4 {
            for x in tx - 4..=tx + 4 {
                let dx = (local.x - x as f64 * h)
                    .abs()
                    .max((local.x - (x + 1) as f64 * h).abs());
                let dy = (local.y - y as f64 * h)
                    .abs()
                    .max((local.y - (y + 1) as f64 * h).abs());
                if dx.hypot(dy) <= self.radius - EPS {
                    self.covered_tiles.insert((x, y));
                }
            }
        }
    }
    fn contains_capsule(&self, a: Point2Dto, b: Point2Dto, r: f64) -> bool {
        if self
            .exterior
            .as_ref()
            .is_some_and(|s| s.capsule_clear(a, b, r))
        {
            return true;
        }
        if r > self.radius {
            return false;
        }
        let (kx, ky) = self.key(a);
        for y in ky - 1..=ky + 1 {
            for x in kx - 1..=kx + 1 {
                if let Some(ids) = self.buckets.get(&(x, y)) {
                    for &id in ids.iter().rev() {
                        let c = self.centers[id];
                        if dist(c, a).max(dist(c, b)) + r <= self.radius + EPS {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
    fn contains(&self, p: Point2Dto) -> bool {
        if self.exterior.as_ref().is_some_and(|s| s.point_clear(p)) {
            return true;
        }
        let h = self.radius * 0.25;
        self.covered_tiles.contains(&(
            ((p.x - self.origin.x) / h).floor() as i32,
            ((p.y - self.origin.y) / h).floor() as i32,
        )) || self.contains_capsule(p, p, 0.0)
    }

    fn contains_cutter_capsule(&self, a: Point2Dto, b: Point2Dto, r: f64) -> bool {
        self.contains_capsule(a, b, (r - self.corner_loss).max(0.0))
    }

    fn contact_arc(&self, c: Point2Dto, r: f64, other: Point2Dto) -> Option<(f64, f64)> {
        let low = disk_contact_arc(c, r - self.corner_loss, other, self.radius)?;
        if self.corner_loss == 0.0 {
            return Some(low);
        }
        let high = disk_contact_arc(c, r, other, self.radius + self.corner_loss)?;

        Some((
            if low.1 >= PI - EPS { high.0 } else { low.0 },
            low.1.min(high.1),
        ))
    }
}

/// Subtract a wrapped angular interval from sorted, disjoint [0, 2pi] ranges.
fn subtract_arc(ranges: &mut Vec<(f64, f64)>, angle: f64, half: f64) {
    if half <= EPS {
        return;
    }
    if half >= PI - EPS {
        ranges.clear();
        return;
    }
    let start = (angle - half).rem_euclid(TAU);
    let end = start + 2.0 * half;
    let cuts = if end <= TAU {
        vec![(start, end)]
    } else {
        vec![(start, TAU), (0.0, end - TAU)]
    };
    for (lo, hi) in cuts {
        let mut next = Vec::with_capacity(ranges.len() + 1);
        for &(a, b) in ranges.iter() {
            if hi <= a || lo >= b {
                next.push((a, b));
            } else {
                if a < lo {
                    next.push((a, lo));
                }
                if hi < b {
                    next.push((hi, b));
                }
            }
        }
        *ranges = next;
    }
}

fn disk_contact_arc(c: Point2Dto, r: f64, other: Point2Dto, radius: f64) -> Option<(f64, f64)> {
    let d = dist(c, other);
    if d + r <= radius + EPS {
        return Some((0.0, PI));
    }
    if d >= r + radius - EPS || d + radius <= r + EPS || d < EPS {
        return None;
    }
    let cosine = ((d * d + r * r - radius * radius) / (2.0 * d * r)).clamp(-1.0, 1.0);
    Some(((other.y - c.y).atan2(other.x - c.x), cosine.acos()))
}

fn angle_with_guard(ranges: &[(f64, f64)]) -> f64 {
    (ranges.iter().map(|&(a, b)| b - a).sum::<f64>()
        + ranges.len() as f64 * 2.0 * TAU / ANGLE_SAMPLES as f64)
        .min(TAU)
}

fn analytic_engagement(
    setup: &CamSetupDto,
    cleared: &Cleared,
    c: Point2Dto,
    r: f64,
    limit: f64,
    work: &mut Work,
) -> Result<f64, CamPlanError> {
    let floor_r = r - cleared.corner_loss;
    let mut ranges = vec![(0.0, TAU)];
    let mut half_plane = |nx: f64, ny: f64, limit: f64| {
        let gap = limit - nx * c.x - ny * c.y;

        let cosine = gap / if gap >= 0.0 { floor_r } else { r };
        if cosine <= -1.0 {
            ranges.clear();
        } else if cosine < 1.0 {
            subtract_arc(&mut ranges, ny.atan2(nx), cosine.acos());
        }
    };
    match setup.resolved_stock {
        CamResolvedStockDto::Box
        | CamResolvedStockDto::ModelBody { .. }
        | CamResolvedStockDto::Rest { .. } => {
            half_plane(1.0, 0.0, setup.stock.max.x);
            half_plane(-1.0, 0.0, -setup.stock.min.x);
            half_plane(0.0, 1.0, setup.stock.max.y);
            half_plane(0.0, -1.0, -setup.stock.min.y);
        }
        CamResolvedStockDto::Cylinder { center, radius } => {
            let critical = (dist(c, center).powi(2) - radius * radius)
                .max(0.0)
                .sqrt()
                .clamp(floor_r, r);
            let contact = [floor_r, r, critical]
                .into_iter()
                .filter_map(|section| disk_contact_arc(c, section, center, radius))
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((a, b)) = contact {
                subtract_arc(&mut ranges, a + PI, PI - b);
            } else {
                ranges.clear();
            }
        }
        CamResolvedStockDto::Hex {
            center,
            across_flats,
        } => {
            for (nx, ny) in [
                (1.0, 0.0),
                (-1.0, 0.0),
                (0.5, 3.0_f64.sqrt() * 0.5),
                (-0.5, 3.0_f64.sqrt() * 0.5),
                (0.5, -3.0_f64.sqrt() * 0.5),
                (-0.5, -3.0_f64.sqrt() * 0.5),
            ] {
                half_plane(nx, ny, across_flats * 0.5 + nx * center.x + ny * center.y);
            }
        }
    }
    if let Some(exterior) = &cleared.exterior {
        work.spend(exterior.query_cost(), 2)?;

        exterior.clip_contact(c, floor_r, &mut ranges);
    }

    if angle_with_guard(&ranges) <= limit {
        return Ok(angle_with_guard(&ranges));
    }
    for &center in cleared.centers.iter().rev().take(16) {
        work.spend(1, 2)?;
        if let Some((a, h)) = cleared.contact_arc(c, r, center) {
            subtract_arc(&mut ranges, a, h);
        }
        if angle_with_guard(&ranges) <= limit {
            return Ok(angle_with_guard(&ranges));
        }
    }
    let (kx, ky) = cleared.key(c);
    let mut keys = Vec::with_capacity(25);
    for y in ky - 2..=ky + 2 {
        for x in kx - 2..=kx + 2 {
            let center = Point2Dto::new(
                cleared.origin.x + (x as f64 + 0.5) * cleared.radius,
                cleared.origin.y + (y as f64 + 0.5) * cleared.radius,
            );
            keys.push((dist(c, center), x, y));
        }
    }
    keys.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    for (_, x, y) in keys {
        if let Some(ids) = cleared.buckets.get(&(x, y)) {
            for &id in ids.iter().rev() {
                work.spend(1, 2)?;
                if let Some((angle, half)) = cleared.contact_arc(c, r, cleared.centers[id]) {
                    subtract_arc(&mut ranges, angle, half);
                    if angle_with_guard(&ranges) <= limit {
                        return Ok(angle_with_guard(&ranges));
                    }
                }
            }
        }
    }
    Ok(angle_with_guard(&ranges))
}

/// Certify an entire candidate disk already empty, including its interior.
/// Subdivision only proves unions of analytic cleared disks or original air;
/// unresolved leaves return false, never a sampled claim of no material.
fn disk_already_clear(setup: &CamSetupDto, cleared: &Cleared, c: Point2Dto, r: f64) -> bool {
    if box_clear(setup, c, r) || cleared.contains_cutter_capsule(c, c, r) {
        return true;
    }

    if cleared.corner_loss > 0.0 {
        return false;
    }
    let mut nodes = 0usize;
    fn visit(
        setup: &CamSetupDto,
        cleared: &Cleared,
        (c, r): (Point2Dto, f64),
        mid: Point2Dto,
        half: f64,
        depth: usize,
        nodes: &mut usize,
    ) -> bool {
        *nodes += 1;
        if *nodes > 4096 {
            return false;
        }
        if mid.x + half <= setup.stock.min.x
            || mid.x - half >= setup.stock.max.x
            || mid.y + half <= setup.stock.min.y
            || mid.y - half >= setup.stock.max.y
        {
            return true;
        }
        let dx = ((mid.x - c.x).abs() - half).max(0.0);
        let dy = ((mid.y - c.y).abs() - half).max(0.0);
        if dx.hypot(dy) >= r + EPS {
            return true;
        }
        if cleared.contains_capsule(mid, mid, half * 2.0_f64.sqrt()) {
            return true;
        }
        if depth == 8 {
            return false;
        }
        let h = half * 0.5;
        [(-h, -h), (h, -h), (-h, h), (h, h)]
            .into_iter()
            .all(|(x, y)| {
                visit(
                    setup,
                    cleared,
                    (c, r),
                    Point2Dto::new(mid.x + x, mid.y + y),
                    h,
                    depth + 1,
                    nodes,
                )
            })
    }
    visit(setup, cleared, (c, r), c, r, 0, &mut nodes)
}

fn box_clear(setup: &CamSetupDto, p: Point2Dto, r: f64) -> bool {
    let dx = (setup.stock.min.x - p.x)
        .max(p.x - setup.stock.max.x)
        .max(0.0);
    let dy = (setup.stock.min.y - p.y)
        .max(p.y - setup.stock.max.y)
        .max(0.0);
    dx.hypot(dy) >= r + EPS
}

fn cutter_clear(setup: &CamSetupDto, cleared: &Cleared, p: Point2Dto, r: f64) -> bool {
    box_clear(setup, p, r) || cleared.contains_cutter_capsule(p, p, r)
}

fn link_clear(setup: &CamSetupDto, cleared: &Cleared, a: Point2Dto, b: Point2Dto, r: f64) -> bool {
    let n = (dist(a, b) / (r * 0.25)).ceil().max(1.0) as usize;
    if n > 4096 {
        return false;
    }
    let mut previous = a;
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let p = Point2Dto::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        let mid = Point2Dto::new((p.x + previous.x) * 0.5, (p.y + previous.y) * 0.5);

        if !cleared.contains_cutter_capsule(previous, p, r)
            && !box_clear(setup, mid, r + dist(previous, p) * 0.5)
        {
            return false;
        }
        previous = p;
    }
    true
}

/// Upper estimate of sampled total engagement, including disjoint sectors.
/// Two angular bins per sector are added for boundary uncertainty. The finite sampling resolution
/// is disclosed; this is not a claim about tooth forces or spindle dynamics.
fn engagement(
    (envelope, setup, cleared): (&Envelope, &CamSetupDto, &Cleared),
    center: Point2Dto,
    r: f64,
    z: f64,
    limit: f64,
    ring: &[Point2Dto],
    work: &mut Work,
) -> Result<f64, CamPlanError> {
    if cleared.corner_loss > 0.0
        || !matches!(
            setup.resolved_stock,
            CamResolvedStockDto::ModelBody { .. } | CamResolvedStockDto::Rest { .. }
        )
    {
        return analytic_engagement(setup, cleared, center, r, limit, work);
    }
    work.spend(ring.len(), 2)?;
    let occupied = ring
        .iter()
        .map(|u| {
            let p = Point2Dto::new(center.x + r * u.x, center.y + r * u.y);
            envelope.initially_occupied(setup, p, z) && !cleared.contains(p)
        })
        .collect::<Vec<_>>();
    Ok(engagement_angle(&occupied))
}

fn engagement_angle(occupied: &[bool]) -> f64 {
    let count = occupied.iter().filter(|&&v| v).count();
    let sectors = (0..occupied.len())
        .filter(|&i| occupied[i] && !occupied[(i + occupied.len() - 1) % occupied.len()])
        .count();
    (count + 2 * sectors).min(occupied.len()) as f64 * TAU / occupied.len() as f64
}

fn lap_engagement(
    (envelope, setup, cleared): (&Envelope, &CamSetupDto, &Cleared),
    (c, q): (Point2Dto, f64),
    r: f64,
    depth: f64,
    limit: f64,
    ring: &[Point2Dto],
    work: &mut Work,
) -> Result<Option<f64>, CamPlanError> {
    let mut maximum: f64 = 0.0;
    for i in 0..LAP_SAMPLES {
        maximum = maximum.max(engagement(
            (envelope, setup, cleared),
            polar(c, q, TAU * i as f64 / LAP_SAMPLES as f64),
            r,
            depth,
            limit,
            ring,
            work,
        )?);
        if maximum > limit + EPS {
            return Ok(None);
        }
    }
    Ok(Some(maximum))
}

fn lap(builder: &mut ProgramBuilder, c: Point2Dto, q: f64, angle: f64, z: f64, feed: f64) {
    for theta in [angle + PI, angle + TAU] {
        let p = polar(c, q, theta);
        builder.circular(Point3Dto::new(p.x, p.y, z), c, false, feed);
    }
}

/// Equal-radius CCW laps share an external tangent. First continue on the
/// already-cleared previous circle, then follow the common tangent into the
/// next lap. The connecting cutter capsule must be fully clear; otherwise
/// retain the axial retract/approach fallback. No engagement limit is relaxed.
fn tangent_link(
    builder: &mut ProgramBuilder,
    setup: &CamSetupDto,
    cleared: &Cleared,
    (previous, next): (Point2Dto, Point2Dto),
    (q, r): (f64, f64),
    z: f64,
    params: &CamAdaptiveParametersDto,
) -> Option<f64> {
    let link = builder.linking.clone();
    if link.as_ref().is_some_and(|l| !l.keep_tool_down) {
        return None;
    }
    let from = builder.position?;
    if (from.z - z).abs() > EPS || (dist(xy(from), previous) - q).abs() > 1e-6 {
        return None;
    }
    let separation = dist(previous, next);
    if separation <= EPS || separation > params.stay_down_distance {
        return None;
    }
    let angle = (next.y - previous.y).atan2(next.x - previous.x) - PI * 0.5;
    let depart = polar(previous, q, angle);
    let arrive = polar(next, q, angle);
    let extra = link.as_ref().map_or(0.0, |l| l.minimum_clearance);
    if !link_clear(setup, cleared, depart, arrive, r + extra) {
        return None;
    }
    let current_angle = (from.y - previous.y).atan2(from.x - previous.x);
    let sweep = (angle - current_angle).rem_euclid(TAU);
    let route = sweep * q + separation;
    let max_length = link.as_ref().map_or(params.stay_down_distance, |l| {
        l.maximum_stay_down * (0.1 + 0.9 * l.stay_down_level as f64 / 100.0)
    });
    if route > max_length {
        return None;
    }
    if extra > 0.0
        && (0..32).any(|i| {
            !link_clear(
                setup,
                cleared,
                polar(previous, q, current_angle + sweep * i as f64 / 32.0),
                polar(previous, q, current_angle + sweep * (i + 1) as f64 / 32.0),
                r + extra + q * (1.0 - (sweep / 64.0).cos()),
            )
        })
    {
        return None;
    }
    let link_feed = link
        .as_ref()
        .map_or(params.linking_feed, |l| l.no_engagement_feed);
    let lift = link.as_ref().map_or(0.0, |l| l.lift_height);
    if z + lift > builder.feed_height_z {
        return None;
    }
    builder.linear(Point3Dto::new(from.x, from.y, z + lift), link_feed);
    if sweep > 1e-8 && TAU - sweep > 1e-8 {
        let pieces = (sweep / PI).ceil() as usize;
        for i in 1..=pieces {
            let p = polar(
                previous,
                q,
                current_angle + sweep * i as f64 / pieces as f64,
            );
            builder.circular(
                Point3Dto::new(p.x, p.y, z + lift),
                previous,
                false,
                link_feed,
            );
        }
    }
    builder.linear(Point3Dto::new(arrive.x, arrive.y, z + lift), link_feed);
    builder.linear(Point3Dto::new(arrive.x, arrive.y, z), link_feed);
    Some(angle)
}

fn configured_ramp(
    builder: &mut ProgramBuilder,
    c: Point2Dto,
    q: f64,
    depth: f64,
    params: &CamAdaptiveParametersDto,
) -> Result<(), CamPlanError> {
    use crate::linking::CamRampType;
    let link = builder.linking.clone().unwrap();
    let r = builder.tool_radius;
    let start_z = (builder.incoming_top + link.ramp_clearance).max(depth);
    if start_z > builder.feed_height_z + EPS {
        return Err(CamPlanError(
            "Ramp clearance reaches above Feed Height. Lower ramp clearance or raise Feed Height."
                .into(),
        ));
    }
    let mut bottom_q = 0.0;
    if link.ramp_type == CamRampType::Helix {
        bottom_q = q - (start_z - depth) * link.ramp_taper_angle.to_radians().tan();
        if q * 2.0 > link.helix_diameter + EPS || bottom_q * 2.0 < link.minimum_helix_diameter - EPS
        {
            return Err(CamPlanError("The required entry helix does not fit the chosen diameter range and taper. Increase the maximum diameter, reduce taper, or reduce minimum cutting radius.".into()));
        }
        let start = polar(c, q, 0.0);
        builder.approach(start, start_z, params.ramp_feed);
        let pitch = link
            .ramp_stepdown
            .min(TAU * bottom_q * link.ramp_angle.to_radians().tan());
        let revolutions = ((start_z - depth) / pitch).ceil().max(1.0) as usize;
        let pieces = if link.ramp_taper_angle == 0.0 { 2 } else { 72 };
        ensure_program_budget(
            builder.commands.len(),
            revolutions.saturating_mul(pieces).saturating_add(4),
            "roughing entry ramp",
        )?;
        for i in 1..=revolutions * pieces {
            let f = i as f64 / (revolutions * pieces) as f64;
            let radius = q + (bottom_q - q) * f;
            let p = polar(c, radius, TAU * i as f64 / pieces as f64);
            let to = Point3Dto::new(p.x, p.y, start_z + (depth - start_z) * f);
            if pieces == 2 {
                builder.circular(to, c, false, link.ramp_feed);
            } else {
                builder.linear(to, link.ramp_feed);
            }
        }
    } else {
        if link.ramp_type == CamRampType::Predrill
            && !builder.predrilled.iter().any(|h| {
                h.bottom <= depth + EPS
                    && dist(h.center, c) + r <= h.radius - 1e-5
                    && link
                        .predrill_positions
                        .iter()
                        .any(|&p| dist(p, h.center) < 1e-5)
            })
        {
            return Err(CamPlanError("Predrill position lacks an earlier enabled hole with sufficient cylindrical depth and diameter.".into()));
        }
        builder.approach(c, depth, link.ramp_feed);
        if link.ramp_type == CamRampType::Plunge {
            let warning="Roughing Plunge entry is full-width axial cutting at Ramp Feed. The tool must be rated for plunging; the radial-load bound applies after entry.";
            if !builder.warnings.iter().any(|w| w == warning) {
                builder.warnings.push(warning.into());
            }
        }
    }

    if bottom_q < q - EPS {
        let turns = ((q - bottom_q) / params.optimal_load).ceil().max(1.0) as usize;
        let count = turns.saturating_mul(72);
        ensure_program_budget(
            builder.commands.len(),
            count.saturating_add(2),
            "roughing entry floor clearing",
        )?;
        for i in 1..=count {
            let radius = bottom_q + (q - bottom_q) * i as f64 / count as f64;
            let point = polar(c, radius, TAU * i as f64 / 72.0);
            builder.linear(Point3Dto::new(point.x, point.y, depth), link.ramp_feed);
        }
    }
    lap(builder, c, q, 0.0, depth, link.ramp_feed);
    Ok(())
}

fn ramp(
    builder: &mut ProgramBuilder,
    c: Point2Dto,
    q: f64,
    depth: f64,
    params: &CamAdaptiveParametersDto,
) -> Result<(), CamPlanError> {
    if builder.linking.is_some() {
        return configured_ramp(builder, c, q, depth, params);
    }
    let start = polar(c, q, 0.0);
    builder.retract_to_clearance();
    builder.rapid(Point3Dto::new(start.x, start.y, builder.clearance_z));
    builder.rapid(Point3Dto::new(start.x, start.y, builder.feed_height_z));
    let pitch = params
        .maximum_ramp_stepdown
        .min(TAU * q * params.ramp_angle_degrees.to_radians().tan());
    let revolutions = ((builder.feed_height_z - depth) / pitch).ceil().max(1.0) as usize;
    ensure_program_budget(
        builder.commands.len(),
        revolutions.saturating_mul(2).saturating_add(2),
        "high-speed roughing helix",
    )?;
    let start_z = builder.feed_height_z;
    for half in 1..=revolutions * 2 {
        let z = start_z + (depth - start_z) * half as f64 / (revolutions * 2) as f64;
        let p = polar(c, q, PI * half as f64);
        builder.circular(Point3Dto::new(p.x, p.y, z), c, false, params.ramp_feed);
    }

    lap(builder, c, q, 0.0, depth, params.ramp_feed);
    Ok(())
}

fn capture_envelope(
    builder: &mut ProgramBuilder,
    setup: &CamSetupDto,
    geometry: &crate::CamAdaptiveGeometryDto,
    tolerance: f64,
    margin: f64,
    work: &mut Work,
) -> Result<Envelope, CamPlanError> {
    let mut envelope = Envelope::new(setup, tolerance, margin)?;
    let mut target = std::mem::take(&mut envelope.target);
    let mut triangles = 0usize;
    for mesh in &geometry.targets {
        triangles = triangles.saturating_add(mesh.indices.len() / 3);
        if triangles > MAX_TRIANGLES {
            return Err(CamPlanError(
                "High Speed Roughing target exceeds its combined triangle budget.".into(),
            ));
        }
        envelope.rasterize(mesh, setup, &mut target, work)?;
    }
    envelope.target = target;
    if let Some(linking) = &builder.linking {
        let mut bounds = setup.stock;
        for mesh in &geometry.targets {
            for v in mesh.positions.as_chunks::<3>().0 {
                let p = [
                    v[0] - setup.wcs.origin.x,
                    v[1] - setup.wcs.origin.y,
                    v[2] - setup.wcs.origin.z,
                ];
                let dot = |a: [f64; 3]| p[0] * a[0] + p[1] * a[1] + p[2] * a[2];
                let (x, y, z) = (
                    dot(setup.wcs.x_axis),
                    dot(setup.wcs.y_axis),
                    dot(setup.wcs.z_axis),
                );
                bounds.min.x = bounds.min.x.min(x);
                bounds.max.x = bounds.max.x.max(x);
                bounds.min.y = bounds.min.y.min(y);
                bounds.max.y = bounds.max.y.max(y);
                bounds.max.z = bounds.max.z.max(z);
            }
        }
        if bounds.max.z + linking.safe_distance > builder.clearance_z {
            return Err(CamPlanError("Roughing Clearance Height must clear all target/stock surfaces plus Safe Distance.".into()));
        }
        builder.link_obstacles = Some(bounds);
    }
    if envelope
        .target
        .iter()
        .any(|&z| z >= builder.clearance_z - EPS)
    {
        return Err(CamPlanError("High Speed Roughing clearance Z must be above all target geometry, not only the stock; raise clearance or repair the setup stock/model selection.".into()));
    }
    if matches!(setup.resolved_stock, CamResolvedStockDto::ModelBody { .. }) {
        let mesh = geometry.stock.as_ref().ok_or_else(|| {
            CamPlanError(
                "High Speed Roughing modeled stock mesh is missing; regenerate the operation."
                    .into(),
            )
        })?;
        let mut heights = vec![f64::NEG_INFINITY; envelope.nx * envelope.ny];
        envelope.rasterize(mesh, setup, &mut heights, work)?;
        envelope.stock = Some(heights);
    }
    if let Some(rest) = &builder.rest_stock {
        envelope.stock = Some(
            (0..envelope.nx * envelope.ny)
                .map(|i| {
                    let p = envelope.center(i);
                    rest.upper_over(
                        [p.x - envelope.h / 2., p.y - envelope.h / 2.],
                        [p.x + envelope.h / 2., p.y + envelope.h / 2.],
                    )
                })
                .collect(),
        );
    }
    Ok(envelope)
}

pub(super) fn plan(
    builder: &mut ProgramBuilder,
    setup: &CamSetupDto,
    operation: &CamOperationDto,
    tool: &CamToolDto,
) -> Result<(), CamPlanError> {
    let mut effective_operation = operation.clone();
    let mut ceiling = f64::INFINITY;
    if let CamOperationDto::Adaptive3d {
        top_z, parameters, ..
    } = &mut effective_operation
    {
        parameters.maximum_stepdown = parameters
            .maximum_stepdown
            .min(tool.flute_length)
            .min(tool.maximum_axial_depth.unwrap_or(tool.flute_length));
        ceiling = *top_z;
        *top_z = top_z.max(builder.incoming_top);
    }
    let operation = &effective_operation;
    let CamOperationDto::Adaptive3d {
        name,
        top_z,
        bottom_z,
        parameters: p,
        geometry,
        cutting,
        ..
    } = operation
    else {
        unreachable!()
    };

    let material_top = builder.incoming_top;
    if builder.feed_height_z < material_top - EPS {
        return Err(CamPlanError(format!("High Speed Roughing '{name}' feed height is below known incoming stock top {material_top:.3} mm; raise feed/retract heights or generate a whole-stock facing operation first. Lowering Top does not remove stock.")));
    }
    let geometry = geometry.as_ref().filter(|g| !g.targets.is_empty()).ok_or_else(||CamPlanError("High Speed Roughing requires current target geometry; regenerate the operation to capture its setup bodies.".into()))?;
    let r = tool.diameter * 0.5;
    let profile = crate::CutterProfile::new(tool.into()).map_err(CamPlanError)?;
    if let Some(link) = builder.linking.as_mut() {
        let straight_length = tool.flute_length - profile.full_radius_height();
        if tool.kind != crate::CamToolKind::FaceMill && straight_length > EPS {
            link.ramp_stepdown = link
                .ramp_stepdown
                .min(straight_length)
                .min(p.maximum_stepdown);
        }
    }
    let floor_r = profile
        .radius_at_height(0.0)
        .expect("validated cutter floor");
    let corner_loss = r - floor_r;
    if tool.kind == crate::CamToolKind::FaceMill {
        return face::plan(builder, setup, operation, tool, geometry, ceiling);
    }
    if material_top - bottom_z > tool.overall_length + EPS {
        return Err(CamPlanError(format!(
            "High Speed Roughing depth {:.3} mm exceeds tool {} overall length {:.3} mm.",
            material_top - bottom_z,
            tool.label(),
            tool.overall_length
        )));
    }
    let q = builder
        .linking
        .as_ref()
        .map_or(p.minimum_cutting_radius, |l| {
            p.minimum_cutting_radius.max(
                l.minimum_helix_diameter / 2.0
                    + (material_top - bottom_z + l.ramp_clearance).max(0.0)
                        * l.ramp_taper_angle.to_radians().tan(),
            )
        });
    if q > floor_r {
        return Err(CamPlanError(format!("The requested ramp/cutting radius {q:.3} mm exceeds the tool's {floor_r:.3} mm flat-land radius and leaves an uncleared center boss. Reduce the ramp diameter/taper or minimum cutting radius, or use a larger flat land.")));
    }
    let swept_radius = r + q;
    let phi = (1.0 - p.optimal_load / r).clamp(-1.0, 1.0).acos();
    let angular_guard = 2.0 * TAU / ANGLE_SAMPLES as f64;
    if phi <= angular_guard * 2.0 {
        return Err(CamPlanError("High Speed Roughing optimal load is too small for the engagement sampling resolution; increase it or use a smaller tool.".into()));
    }

    let beta = (phi - angular_guard) * 0.5;
    let floor_sweep = floor_r + q;
    let d = (floor_sweep * floor_sweep - floor_r * floor_r * beta.sin().powi(2)).sqrt()
        - floor_r * beta.cos();
    let pitch = (0.9 * (d - q)).max(1.0e-6);
    let mut work = Work::default();
    let envelope = capture_envelope(
        builder,
        setup,
        geometry,
        p.tolerance,
        2.0 * (swept_radius + p.radial_stock_to_leave + p.tolerance),
        &mut work,
    )?;
    let origin = Point2Dto::new(
        setup.stock.min.x - swept_radius - pitch,
        setup.stock.min.y - swept_radius - pitch,
    );
    let nx = ((setup.stock.max.x - setup.stock.min.x + 2.0 * (swept_radius + pitch)) / pitch).ceil()
        as usize
        + 1;
    let ny = ((setup.stock.max.y - setup.stock.min.y + 2.0 * (swept_radius + pitch)) / pitch).ceil()
        as usize
        + 1;
    if nx.saturating_mul(ny) > MAX_PATCHES {
        return Err(CamPlanError("High Speed Roughing patch frontier exceeds its memory budget; increase optimal load or reduce the setup bounds.".into()));
    }
    let center = |i: usize| {
        Point2Dto::new(
            origin.x + (i % nx) as f64 * pitch,
            origin.y + (i / nx) as f64 * pitch,
        )
    };
    let neighbors = |i: usize| {
        [
            (!i.is_multiple_of(nx)).then(|| i - 1),
            (i % nx + 1 < nx).then(|| i + 1),
            (i / nx > 0).then(|| i - nx),
            (i / nx + 1 < ny).then(|| i + nx),
        ]
    };
    let ring = (0..ANGLE_SAMPLES)
        .map(|i| {
            polar(
                Point2Dto::new(0.0, 0.0),
                1.0,
                TAU * i as f64 / ANGLE_SAMPLES as f64,
            )
        })
        .collect::<Vec<_>>();
    let corner_height = profile.full_radius_height();
    let corner_overlap = if corner_height + EPS >= p.maximum_stepdown
        && p.maximum_stepdown + corner_height <= tool.flute_length + EPS
    {
        corner_height
    } else {
        0.0
    };
    let depths = layers::depth_order(
        setup,
        &geometry.targets,
        *top_z,
        *bottom_z,
        ceiling,
        p,
        corner_height,
    )?;
    let mut history = Vec::<layers::Removal>::new();

    let mut full_radius_history = Vec::<layers::Removal>::new();
    let mut total_laps = 0usize;
    let mut exterior_passes = 0usize;
    let mut cavity_entries = 0usize;
    let mut max_engagement: f64 = 0.0;
    let mut inaccessible_cells = 0usize;
    let safety_radius = swept_radius + p.radial_stock_to_leave + 2.0_f64.sqrt() * envelope.h;
    for depth in depths {
        work.spend(envelope.target.len() * 2 + nx * ny, 1)?;
        let distances = envelope.clearance(depth, p.axial_stock_to_leave);
        let safe = (0..nx * ny)
            .map(|i| {
                envelope
                    .index(center(i))
                    .is_some_and(|j| distances[j] >= safety_radius * safety_radius)
            })
            .collect::<Vec<_>>();

        work.spend(nx * ny * 8, 1)?;
        let boundary_front = {
            let adjacent = (0..nx * ny)
                .map(|i| !safe[i] || neighbors(i).into_iter().flatten().any(|j| !safe[j]))
                .collect::<Vec<_>>();
            (0..nx * ny)
                .map(|i| {
                    safe[i]
                        && (adjacent[i] || neighbors(i).into_iter().flatten().any(|j| adjacent[j]))
                })
                .collect::<Vec<_>>()
        };
        let mut remaining = Material::new(&envelope, setup, depth);
        let mut cleared = Cleared::new(floor_sweep, xy(setup.stock.min));
        cleared.corner_loss = corner_loss;
        layers::restore(&history, depth, &mut cleared, &mut work)?;
        let inherited_centers = cleared.centers.len();
        for &c in &cleared.centers {
            mark_cleared(&envelope, &mut remaining, c, floor_sweep, &mut work)?;
        }

        let axial_check = depth + p.maximum_stepdown < material_top - EPS;
        let mut upper = Cleared::new(swept_radius, xy(setup.stock.min));
        if axial_check {
            layers::restore(
                &full_radius_history,
                depth + p.maximum_stepdown + corner_overlap,
                &mut upper,
                &mut work,
            )?;
        }
        let mut full_radius_exterior = None;
        if let Some(mut front) = ConvexStock::from_envelope(
            &envelope,
            depth,
            p.axial_stock_to_leave,
            p.radial_stock_to_leave,
            &mut work,
        )? {
            front = front.refine_circular(
                setup,
                &geometry.targets,
                depth,
                p.axial_stock_to_leave,
                p.tolerance,
                &mut work,
            )?;
            if !axial_check
                || upper
                    .exterior
                    .as_ref()
                    .is_some_and(|prior| front.contains_bound(prior))
            {
                let prior_bounds = history
                    .iter()
                    .chain(full_radius_history.iter())
                    .filter(|cut| cut.depth <= depth + EPS)
                    .filter_map(|cut| cut.exterior.as_ref())
                    .collect::<Vec<_>>();
                exterior_passes += front.clear_exterior(
                    builder,
                    setup,
                    (r, floor_r, depth),
                    p,
                    (cutting.feed_xy, cutting.feed_z),
                    (&mut work, &envelope, &prior_bounds),
                )?;
                front.mark_completed_cap(floor_r, p);
                full_radius_exterior = Some(front.clone());

                front.offset += corner_loss;
                cleared.exterior = Some(front);
            }
        }
        if let Some(front) = &cleared.exterior {
            work.spend(
                remaining
                    .cells
                    .iter()
                    .filter(|&&occupied| occupied)
                    .count()
                    .saturating_mul(front.query_cost())
                    .saturating_mul(2),
                3,
            )?;
            for i in 0..remaining.cells.len() {
                if remaining.cells[i]
                    && front.distance(envelope.center(i))
                        > front.offset + envelope.h * std::f64::consts::FRAC_1_SQRT_2 + EPS
                {
                    remaining.cells[i] = false;
                    let (x, y) = (i % envelope.nx, i / envelope.nx);
                    remaining.counts
                        [x / MATERIAL_TILE + remaining.tiles_x * (y / MATERIAL_TILE)] -= 1;
                }
            }
        }
        let mut component_seen = vec![false; nx * ny];
        let mut reached = vec![false; nx * ny];
        let mut queued = vec![false; nx * ny];
        let mut attempts = vec![0u8; nx * ny];
        let mut previous_lap = None;
        for component_seed in 0..nx * ny {
            if !safe[component_seed] || component_seen[component_seed] {
                continue;
            }
            let mut component = Vec::new();
            let mut flood = VecDeque::from([component_seed]);
            component_seen[component_seed] = true;
            while let Some(i) = flood.pop_front() {
                component.push(i);
                for j in neighbors(i).into_iter().flatten() {
                    if safe[j] && !component_seen[j] {
                        component_seen[j] = true;
                        flood.push_back(j);
                    }
                }
            }

            let air = component
                .iter()
                .copied()
                .filter(|&i| box_clear(setup, center(i), swept_radius))
                .min_by(|&a, &b| {
                    let hint = builder
                        .linking
                        .as_ref()
                        .and_then(|l| l.entry_positions.first())
                        .copied()
                        .unwrap_or(center(component[0]));
                    dist(center(a), hint).total_cmp(&dist(center(b), hint))
                });
            let reused = component
                .iter()
                .copied()
                .find(|&i| cleared.contains_cutter_capsule(center(i), center(i), swept_radius));
            let seed = if let Some(air) = air.or(reused) {
                air
            } else {
                if !p.machine_cavities {
                    continue;
                }
                if builder
                    .linking
                    .as_ref()
                    .is_some_and(|l| l.ramp_type == crate::linking::CamRampType::Predrill)
                {
                    let link = builder.linking.as_ref().unwrap();
                    *component.iter().filter(|&&i|builder.predrilled.iter().any(|h|h.bottom<=depth+EPS
                        && dist(h.center,center(i))+r<=h.radius-1e-5
                        && link.predrill_positions.iter().any(|p|dist(*p,h.center)<1e-5)))
                        .min_by(|&&a,&&b|{
                            let score=|i|builder.predrilled.iter().map(|h|dist(h.center,center(i))).fold(f64::INFINITY,f64::min);
                            score(a).total_cmp(&score(b))
                        }).ok_or_else(||CamPlanError("No selected earlier predrill clears this cavity entry to full cutter diameter and depth. Use Helix, move drilling earlier, or enlarge/deepen the predrill.".into()))?
                } else {
                    *component
                        .iter()
                        .max_by(|&&a, &&b| {
                            if let Some(hint) = builder
                                .linking
                                .as_ref()
                                .and_then(|l| l.entry_positions.first())
                            {
                                return dist(center(b), *hint).total_cmp(&dist(center(a), *hint));
                            }
                            let da = envelope.index(center(a)).map_or(0.0, |i| distances[i]);
                            let db = envelope.index(center(b)).map_or(0.0, |i| distances[i]);
                            da.total_cmp(&db)
                        })
                        .unwrap()
                }
            };
            if air.is_none() && reused.is_none() {
                ramp(builder, center(seed), q, depth, p)?;
                previous_lap = Some(center(seed));
                cleared.add(center(seed));
                mark_cleared(
                    &envelope,
                    &mut remaining,
                    center(seed),
                    floor_sweep,
                    &mut work,
                )?;
                total_laps += 1;
                cavity_entries += 1;

                if axial_check {
                    upper.add(center(seed));
                }
            }

            let row_stride = (swept_radius / pitch).floor().max(1.0) as usize;
            let on_front = |i: usize| {
                (i / nx) % row_stride == (seed / nx) % row_stride
                    || i % nx == seed % nx
                    || boundary_front[i]
                    || box_clear(setup, center(i), swept_radius)
            };
            let mut queue = VecDeque::from([(seed, seed)]);
            queued[seed] = true;

            while let Some((i, parent)) = queue.pop_back() {
                queued[i] = false;
                if reached[i] {
                    continue;
                }
                work.spend(1, 1)?;
                attempts[i] += 1;
                let c = center(i);
                if has_material(&envelope, &remaining, c, swept_radius, &mut work)?
                    && !disk_already_clear(setup, &cleared, c, swept_radius)
                {
                    if axial_check && !upper.contains_capsule(c, c, swept_radius) {
                        continue;
                    }
                    let Some(load) = lap_engagement(
                        (&envelope, setup, &cleared),
                        (c, q),
                        r,
                        depth,
                        phi,
                        &ring,
                        &mut work,
                    )?
                    else {
                        continue;
                    };
                    let pc = center(parent);
                    let preferred = (pc.y - c.y).atan2(pc.x - c.x);
                    let angle = (0..16)
                        .map(|k| preferred + TAU * k as f64 / 16.0)
                        .find(|&a| cutter_clear(setup, &cleared, polar(c, q, a), r));
                    let Some(angle) = angle else {
                        continue;
                    };
                    ensure_program_budget(builder.commands.len(), 16, name)?;
                    let tangent_angle = previous_lap.and_then(|previous| {
                        tangent_link(builder, setup, &cleared, (previous, c), (q, r), depth, p)
                    });
                    let angle = tangent_angle.unwrap_or(angle);
                    if tangent_angle.is_none() {
                        linking::exit_lap(builder, setup, &cleared, previous_lap, q, r, depth)?;
                        let start = polar(c, q, angle);
                        if !linking::roll(
                            builder,
                            setup,
                            &cleared,
                            (start, Point2Dto::new(-angle.sin(), angle.cos())),
                            (depth, r),
                            true,
                            cutting.feed_z,
                        )? {
                            builder.approach(start, depth, cutting.feed_z);
                        }
                    }
                    lap(builder, c, q, angle, depth, cutting.feed_xy);
                    previous_lap = Some(c);
                    max_engagement = max_engagement.max(load);
                    total_laps += 1;
                    cleared.add(c);
                    mark_cleared(&envelope, &mut remaining, c, floor_sweep, &mut work)?;
                }
                reached[i] = true;
                for j in neighbors(i).into_iter().flatten() {
                    if safe[j] && on_front(j) && !reached[j] && !queued[j] && attempts[j] < 4 {
                        queue.push_back((j, i));
                        queued[j] = true;
                    }
                }
            }
        }

        inaccessible_cells += remaining
            .cells
            .iter()
            .enumerate()
            .filter(|&(i, &occupied)| {
                occupied && envelope.target[i] + p.axial_stock_to_leave < depth - EPS
            })
            .count();
        linking::exit_lap(builder, setup, &cleared, previous_lap, q, r, depth)?;
        builder.retract_to_clearance();
        history.push(layers::Removal {
            depth,
            exterior: cleared.exterior.clone(),
            centers: cleared.centers[inherited_centers..].to_vec(),
        });
        full_radius_history.push(layers::Removal {
            depth: depth + corner_height,
            exterior: full_radius_exterior,
            centers: cleared.centers[inherited_centers..].to_vec(),
        });
    }
    if total_laps == 0 && exterior_passes == 0 {
        if builder.rest_stock.is_some() {
            builder.warnings.push(format!("High Speed Roughing '{name}' found no remaining stock to cut; the operation is empty."));
            return Ok(());
        }
        return Err(CamPlanError("High Speed Roughing found no accessible cutting area at these allowances, tool radius, and engagement limit.".into()));
    }
    builder.warnings.push(format!("High Speed Roughing '{name}': {exterior_passes} continuous exterior passes, {total_laps} fallback rounded laps, {cavity_entries} helical entries. Exterior side-cut advance <= {:.4} mm with a continuous {:.2}° engagement bound; fallback sampled maximum {:.2}° / {:.2}° limit. Entry ramps use their separate pitch/feed limits.",p.optimal_load,phi.to_degrees(),max_engagement.to_degrees(),phi.to_degrees()));
    builder.warnings.push(format!("High Speed Roughing target envelope: {:.4} mm whole protected cells. Circular exterior passes enclose target triangles directly; other exterior passes enclose protected cells. Fallback patches add {:.4} mm conservative radial sampling margin. Fallback engagement uses analytic standard-stock intervals ({} angles for modeled stock) at {} positions per lap. Concavities/cavities retain the conservative fallback; narrow corners/undercuts require subsequent operations.",envelope.h,2.0_f64.sqrt()*envelope.h,ANGLE_SAMPLES,LAP_SAMPLES));
    if inaccessible_cells > 0 {
        builder.warnings.push(format!("High Speed Roughing retained {inaccessible_cells} potentially machinable cell-level samples across depth levels (allowance bands, small corners, inaccessible regions, or engagement-limited fronts). Inspect remaining stock; this operation does not claim complete clearing."));
    }
    if corner_loss > EPS {
        builder.warnings.push(format!("Corner-profile roughing uses a {:.3} mm flat cutting diameter for floor-stock and engagement proofs; outer diameter still protects the target. Rounded/beveled floor stock and shallow-cut cusps remain material in simulation. Modeled-stock engagement uses its conservative bounding box. No automatic corner finishing is implied.", 2.0 * floor_r));
    }
    builder.warnings.push("High Speed Roughing takes deep major cuts within Ap, then steps upward through terraces using the remaining stock. Target and axial clearance must be proved for each cut. Completed exterior and cavity cuts reduce stock at higher levels. Rounded tools overlap major bands by the corner height and require full-diameter clearance above Ap; stepdowns no larger than the corner height retain terrace ordering. XY stock is conservative; incoming top comes from the transferred rest volume or a proved whole-stock facing pass. Rest-from-setup uses a simulated upper stock envelope; same-setup removal beyond proved facing and holder/fixture checks are not implemented.".into());
    Ok(())
}

fn cell_range(e: &Envelope, c: Point2Dto, r: f64) -> (usize, usize, usize, usize) {
    let bound = |v: f64, min: f64, n: usize| {
        (((v - min) / e.h).floor() as isize).clamp(0, n as isize - 1) as usize
    };
    (
        bound(c.x - r, e.min.x, e.nx),
        bound(c.x + r, e.min.x, e.nx),
        bound(c.y - r, e.min.y, e.ny),
        bound(c.y + r, e.min.y, e.ny),
    )
}

const MATERIAL_TILE: usize = 16;
struct Material {
    cells: Vec<bool>,
    counts: Vec<usize>,
    tiles_x: usize,
}
impl Material {
    fn new(e: &Envelope, setup: &CamSetupDto, depth: f64) -> Self {
        let tiles_x = e.nx.div_ceil(MATERIAL_TILE);
        let mut result = Self {
            cells: vec![false; e.target.len()],
            counts: vec![0; tiles_x * e.ny.div_ceil(MATERIAL_TILE)],
            tiles_x,
        };
        for i in 0..result.cells.len() {
            let p = e.center(i);
            if depth < setup.stock.max.z
                && p.x + e.h * 0.5 >= setup.stock.min.x
                && p.x - e.h * 0.5 <= setup.stock.max.x
                && p.y + e.h * 0.5 >= setup.stock.min.y
                && p.y - e.h * 0.5 <= setup.stock.max.y
            {
                result.cells[i] = true;
                result.counts[i % e.nx / MATERIAL_TILE + tiles_x * (i / e.nx / MATERIAL_TILE)] += 1;
            }
        }
        result
    }
}

fn has_material(
    e: &Envelope,
    remaining: &Material,
    c: Point2Dto,
    r: f64,
    work: &mut Work,
) -> Result<bool, CamPlanError> {
    let r = r + e.h * std::f64::consts::FRAC_1_SQRT_2;
    let (x0, x1, y0, y1) = cell_range(e, c, r);
    for ty in y0 / MATERIAL_TILE..=y1 / MATERIAL_TILE {
        for tx in x0 / MATERIAL_TILE..=x1 / MATERIAL_TILE {
            if remaining.counts[tx + remaining.tiles_x * ty] == 0 {
                continue;
            }
            for y in y0.max(ty * MATERIAL_TILE)..=y1.min((ty + 1) * MATERIAL_TILE - 1) {
                for x in x0.max(tx * MATERIAL_TILE)..=x1.min((tx + 1) * MATERIAL_TILE - 1) {
                    work.spend(1, 3)?;
                    let i = x + e.nx * y;
                    if remaining.cells[i] && dist(e.center(i), c) < r - EPS {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

fn mark_cleared(
    e: &Envelope,
    remaining: &mut Material,
    c: Point2Dto,
    r: f64,
    work: &mut Work,
) -> Result<(), CamPlanError> {
    let (x0, x1, y0, y1) = cell_range(e, c, r);

    for ty in y0 / MATERIAL_TILE..=y1 / MATERIAL_TILE {
        for tx in x0 / MATERIAL_TILE..=x1 / MATERIAL_TILE {
            let tile = tx + remaining.tiles_x * ty;
            work.spend(1, 3)?;
            if remaining.counts[tile] == 0 {
                continue;
            }
            let (xa, xb) = (
                x0.max(tx * MATERIAL_TILE),
                x1.min((tx + 1) * MATERIAL_TILE - 1),
            );
            let (ya, yb) = (
                y0.max(ty * MATERIAL_TILE),
                y1.min((ty + 1) * MATERIAL_TILE - 1),
            );
            work.spend((xb - xa + 1) * (yb - ya + 1), 3)?;
            for y in ya..=yb {
                for x in xa..=xb {
                    let i = x + e.nx * y;

                    if remaining.cells[i]
                        && dist(e.center(i), c) + e.h * std::f64::consts::FRAC_1_SQRT_2 <= r - EPS
                    {
                        remaining.cells[i] = false;
                        remaining.counts[tile] -= 1;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("adaptive/linking_tests.rs");
    include!("adaptive/spiral_tests.rs");
    include!("adaptive/layer_tests.rs");
    include!("adaptive/corner_tests.rs");
    include!("adaptive/rest_tests.rs");
    use super::*;
    use crate::model::*;
    use crate::planner::{plan_setup, CamCommandDto};

    fn path_distance_to_segment(
        from: Point3Dto,
        cmd: &CamCommandDto,
        a: Point2Dto,
        b: Point2Dto,
    ) -> f64 {
        use super::super::{arc_segment_distance, segment_segment_distance, LeadArc};
        let to = cmd.endpoint().unwrap();
        match cmd {
            CamCommandDto::Circular {
                center, clockwise, ..
            } => arc_segment_distance(
                xy(from),
                &LeadArc {
                    center: xy(*center),
                    clockwise: *clockwise,
                    arc_end: xy(to),
                },
                a,
                b,
            ),
            _ => segment_segment_distance(xy(from), xy(to), a, b),
        }
    }

    fn assert_continuous_box_clearance(
        program: &crate::planner::CamProgramDto,
        min: Point2Dto,
        max: Point2Dto,
        top: f64,
        radius: f64,
    ) {
        let edges = [
            min,
            Point2Dto::new(max.x, min.y),
            max,
            Point2Dto::new(min.x, max.y),
        ];
        assert_continuous_polygon_clearance(program, &edges, top, radius);
    }

    fn assert_continuous_polygon_clearance(
        program: &crate::planner::CamProgramDto,
        edges: &[Point2Dto],
        top: f64,
        radius: f64,
    ) {
        let mut previous: Option<Point3Dto> = None;
        for cmd in &program.commands {
            if let Some(to) = cmd.endpoint() {
                if let Some(from) = previous.filter(|from| from.z.min(to.z) < top - EPS) {
                    assert!(!super::super::point_in_polygon(xy(from), edges));
                    assert!(!super::super::point_in_polygon(xy(to), edges));
                    for i in 0..edges.len() {
                        assert!(
                            path_distance_to_segment(
                                from,
                                cmd,
                                edges[i],
                                edges[(i + 1) % edges.len()]
                            ) >= radius - 1e-7,
                            "continuous swept cutter violates analytic polygon clearance: {cmd:?}"
                        );
                    }
                }
                previous = Some(to);
            }
        }
    }

    fn point_is_cut_at_depth(
        program: &crate::planner::CamProgramDto,
        p: Point2Dto,
        depth: f64,
        radius: f64,
    ) -> bool {
        let mut previous: Option<Point3Dto> = None;
        for cmd in &program.commands {
            if let Some(to) = cmd.endpoint() {
                if let Some(from) =
                    previous.filter(|from| from.z <= depth + EPS && to.z <= depth + EPS)
                {
                    if matches!(
                        cmd,
                        CamCommandDto::Linear { .. } | CamCommandDto::Circular { .. }
                    ) && path_distance_to_segment(from, cmd, p, p) <= radius
                    {
                        return true;
                    }
                }
                previous = Some(to);
            }
        }
        false
    }

    fn cuboid(min: [f64; 3], max: [f64; 3]) -> CamStockMeshDto {
        CamStockMeshDto {
            positions: [
                [min[0], min[1], min[2]],
                [max[0], min[1], min[2]],
                [max[0], max[1], min[2]],
                [min[0], max[1], min[2]],
                [min[0], min[1], max[2]],
                [max[0], min[1], max[2]],
                [max[0], max[1], max[2]],
                [min[0], max[1], max[2]],
            ]
            .into_iter()
            .flatten()
            .collect(),
            indices: vec![
                0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2,
                7, 6, 3, 0, 4, 3, 4, 7,
            ],
        }
    }

    fn fixture(targets: Vec<CamStockMeshDto>) -> CamDocumentDto {
        let cutting = CuttingParametersDto {
            spindle_rpm: 8000,
            feed_xy: 600.0,
            feed_z: 100.0,
            coolant: CoolantMode::Flood,
        };
        let operation = CamOperationDto::Adaptive3d {
            id: 1,
            name: "Adaptive test".into(),
            enabled: true,
            tool_id: 1,
            top_z: 0.0,
            bottom_z: -2.0,
            clearance_z: 5.0,
            retract_z: 3.0,
            feed_height_z: 1.0,
            cutting,
            parameters: CamAdaptiveParametersDto {
                optimal_load: 1.0,
                maximum_stepdown: 1.0,
                minimum_cutting_radius: 0.8,
                radial_stock_to_leave: 0.1,
                axial_stock_to_leave: 0.1,
                tolerance: 0.2,
                ramp_angle_degrees: 3.0,
                maximum_ramp_stepdown: 0.5,
                ramp_feed: 100.0,
                linking_feed: 600.0,
                stay_down_distance: 8.0,
                machine_cavities: true,
            },
            geometry: Some(CamAdaptiveGeometryDto {
                targets,
                stock: None,
            }),
        };
        let mut doc = CamDocumentDto::default();
        doc.tools.push(CamToolDto {
            id: 1,
            number: Some(1),
            name: "4mm flat".into(),
            kind: CamToolKind::FlatEndMill,
            diameter: 4.0,
            flute_length: 10.0,
            overall_length: 30.0,
            center_cutting: true,
            flute_count: 3,
            point_angle_degrees: None,
            corner_radius: None,
            corner_chamfer: None,
            cutting,
            cutting_presets: vec![],
            maximum_axial_depth: None,
            default_step_down: None,
            default_step_over: None,
        });
        doc.setups.push(CamSetupDto {
            id: 1,
            name: "Roughing".into(),
            wcs: WorkCoordinateSystemDto::default(),
            wcs_origin: WcsOriginSpecDto::Explicit,
            work_offset: WorkOffset::G54,
            work_offset_count: 1,
            stock_spec: CamStockSpecDto::LegacyBox,
            resolved_stock: CamResolvedStockDto::Box,
            stock: StockBoxDto {
                min: Point3Dto::new(0.0, 0.0, -3.0),
                max: Point3Dto::new(16.0, 14.0, 0.0),
            },
            stock_model_box: None,
            body_ids: vec![],
            machine: None,
            legacy_clearance_z: None,
            legacy_retract_z: None,
            operations: vec![operation],
        });
        doc.active_setup_id = Some(1);
        doc.next_setup_id = 2;
        doc.next_operation_id = 2;
        doc.next_tool_id = 2;
        doc
    }

    fn cavity_fixture() -> CamDocumentDto {
        let mut doc = fixture(vec![
            cuboid([0.0, 0.0, -3.0], [16.0, 2.0, 0.0]),
            cuboid([0.0, 12.0, -3.0], [16.0, 14.0, 0.0]),
            cuboid([0.0, 2.0, -3.0], [2.0, 12.0, 0.0]),
            cuboid([14.0, 2.0, -3.0], [16.0, 12.0, 0.0]),
            cuboid([0.0, 0.0, -3.0], [16.0, 14.0, -2.5]),
        ]);
        let CamOperationDto::Adaptive3d { bottom_z, .. } = &mut doc.setups[0].operations[0] else {
            unreachable!()
        };
        *bottom_z = -1.0;
        doc
    }

    #[test]
    fn tiled_material_updates_equal_the_dense_whole_cell_predicate() {
        let doc = fixture(vec![]);
        let setup = &doc.setups[0];
        let envelope = Envelope::new(setup, 0.2, 6.0).unwrap();
        let mut material = Material::new(&envelope, setup, -1.0);
        let mut dense = material.cells.clone();
        let mut work = Work::default();
        for i in 0..70 {
            let c = Point2Dto::new(-2.0 + i as f64 * 0.3, 7.0 + 4.0 * (i as f64 * 0.2).sin());
            mark_cleared(&envelope, &mut material, c, 2.8, &mut work).unwrap();
            for (j, cell) in dense.iter_mut().enumerate() {
                if dist(envelope.center(j), c) + envelope.h * std::f64::consts::FRAC_1_SQRT_2
                    <= 2.8 - EPS
                {
                    *cell = false;
                }
            }
            assert_eq!(material.cells, dense);
            assert_eq!(
                material.counts.iter().sum::<usize>(),
                dense.iter().filter(|&&occupied| occupied).count()
            );
        }
    }

    #[test]
    fn adaptive_thin_external_stock_clears_all_four_wall_bands() {
        let mut doc = fixture(vec![cuboid([2.0, 2.0, 2.0], [32.0, 17.0, 12.0])]);
        doc.setups[0].stock.min = Point3Dto::new(0.0, 0.0, 0.0);
        doc.setups[0].stock.max = Point3Dto::new(34.0, 19.0, 12.0);
        doc.tools[0].diameter = 12.0;
        doc.tools[0].flute_length = 24.0;
        let CamOperationDto::Adaptive3d {
            top_z,
            bottom_z,
            clearance_z,
            retract_z,
            feed_height_z,
            parameters,
            ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };

        *top_z = 12.0;
        *bottom_z = 0.0;
        *clearance_z = 24.0;
        *retract_z = 19.0;
        *feed_height_z = 16.0;
        parameters.maximum_stepdown = 10.0;
        parameters.optimal_load = 1.2;
        parameters.minimum_cutting_radius = 2.4;
        parameters.radial_stock_to_leave = 0.2;
        parameters.axial_stock_to_leave = 0.2;
        let program = plan_setup(&doc, 1).unwrap();

        assert_continuous_box_clearance(
            &program,
            Point2Dto::new(2.0, 2.0),
            Point2Dto::new(32.0, 17.0),
            12.0,
            6.2,
        );
        let points = (4..=30)
            .flat_map(|x| {
                [
                    Point2Dto::new(x as f64, 0.7),
                    Point2Dto::new(x as f64, 18.3),
                ]
            })
            .chain((4..=15).flat_map(|y| {
                [
                    Point2Dto::new(0.7, y as f64),
                    Point2Dto::new(33.3, y as f64),
                ]
            }));
        let missed = points
            .filter(|&p| !point_is_cut_at_depth(&program, p, 2.0, 6.0))
            .collect::<Vec<_>>();
        assert!(
            missed.is_empty(),
            "reachable stock between roughing bands: {missed:?}; {:?}",
            program.warnings
        );
        assert!(
            program.commands.len() < 400,
            "exterior must not regress to fine-grid laps"
        );
        assert!(program.stats.cutting_distance < 1500.0);
        eprintln!(
            "thin-stock coverage: {} commands; {:?}",
            program.commands.len(),
            program.warnings.first()
        );
    }

    fn face_fixture() -> CamDocumentDto {
        let mut doc = fixture(vec![
            cuboid([2.0, 2.0, -3.0], [14.0, 12.0, -1.5]),
            cuboid([6.0, 5.0, -1.5], [10.0, 9.0, -0.5]),
        ]);
        let tool = &mut doc.tools[0];
        tool.kind = CamToolKind::FaceMill;
        tool.diameter = 16.0;
        tool.flute_count = 2;
        tool.center_cutting = false;
        tool.flute_length = 1.0;
        tool.maximum_axial_depth = Some(1.0);
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        parameters.maximum_stepdown = 0.8;
        parameters.optimal_load = 16.0;
        doc
    }

    #[test]
    fn face_roughing_shallow_layers_clear_stock_without_body_contact() {
        use crate::{simulate_setup, CamSimulationRequestDto, CamSimulationTargetDto};
        for (corner, length) in [(None, 1.0), (Some(0.2), 1.0), (Some(1.5), 2.5)] {
            let mut doc = face_fixture();
            doc.tools[0].corner_radius = corner;
            doc.tools[0].flute_length = length;
            let program = plan_setup(&doc, 1).unwrap();
            assert!(program.warnings[0].contains("0 helical entries"));
            assert!(program.warnings[0].contains("Maximum Ap 1.000 mm"));
            let CamOperationDto::Adaptive3d {
                geometry: Some(g), ..
            } = &doc.setups[0].operations[0]
            else {
                unreachable!()
            };
            let request = CamSimulationRequestDto {
                setup_id: 1,
                voxel_size: Some(0.25),
                max_voxels: None,
                stock_mesh: None,
                target: Some(CamSimulationTargetDto {
                    cache_key: None,
                    meshes: g.targets.clone(),
                    tolerance_mm: 0.05,
                }),
                through_operation_id: None,
                completed_steps: None,
                playback_time_seconds: None,
            };
            let short = simulate_setup(&doc, &request).unwrap();
            assert!(short.removed_volume_mm3 > 150.0);
            assert!(short.collisions.is_empty(), "{:?}", short.collisions);
            assert_eq!(short.comparison.as_ref().unwrap().gouged_voxels, 0);

            doc.tools[0].flute_length = 20.0;
            assert_eq!(program.commands, plan_setup(&doc, 1).unwrap().commands);
            let body = simulate_setup(&doc, &request).unwrap();
            assert_eq!(
                short
                    .steps
                    .iter()
                    .map(|s| s.removed_voxels)
                    .collect::<Vec<_>>(),
                body.steps
                    .iter()
                    .map(|s| s.removed_voxels)
                    .collect::<Vec<_>>()
            );
            assert_eq!(short.remaining_voxels, body.remaining_voxels);
            assert_eq!(body.comparison.as_ref().unwrap().gouged_voxels, 0);

            if corner.is_none() {
                assert!(short.stock_mesh == body.stock_mesh);
            }
        }
    }

    #[test]
    fn face_roughing_enforces_ap_from_tool_data_independently_of_programming_radius() {
        let mut doc = face_fixture();
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        parameters.maximum_stepdown = 1.1;
        let clamped = plan_setup(&doc, 1).unwrap();
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        parameters.maximum_stepdown = 1.0;
        assert_eq!(clamped.commands, plan_setup(&doc, 1).unwrap().commands);
        let mut doc = face_fixture();
        for radius in [0.4, 1.2, 1.5] {
            doc.tools[0].corner_radius = Some(radius);
            assert!(plan_setup(&doc, 1).is_ok());
        }
        assert_eq!(doc.tools[0].flute_length, 1.0);
        let mut doc = face_fixture();
        doc.tools[0].maximum_axial_depth = Some(f64::NAN);
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("maximum axial depth"));
        doc.tools[0].maximum_axial_depth = None;
        assert!(plan_setup(&doc, 1).is_ok());
    }

    #[test]
    fn high_feed_face_insert_with_one_mm_flute_generates_and_simulates() {
        for radius in [1.2, 1.5] {
            let mut doc = face_fixture();
            doc.tools[0].corner_radius = Some(radius);
            let program = plan_setup(&doc, 1).unwrap();
            assert!(program
                .warnings
                .iter()
                .any(|w| w.contains("Maximum Ap 1.000 mm")));
            let CamOperationDto::Adaptive3d {
                geometry: Some(g), ..
            } = &doc.setups[0].operations[0]
            else {
                unreachable!()
            };
            let result = crate::simulate_setup(
                &doc,
                &crate::CamSimulationRequestDto {
                    setup_id: 1,
                    voxel_size: Some(0.2),
                    max_voxels: None,
                    stock_mesh: None,
                    target: Some(crate::CamSimulationTargetDto {
                        cache_key: None,
                        meshes: g.targets.clone(),
                        tolerance_mm: 0.05,
                    }),
                    through_operation_id: None,
                    completed_steps: None,
                    playback_time_seconds: None,
                },
            )
            .unwrap();
            assert!(result.removed_volume_mm3 > 150.);
            assert!(result.collisions.is_empty());
            assert_eq!(result.comparison.unwrap().gouged_voxels, 0);
            let restored: CamDocumentDto =
                serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
            assert_eq!(restored.tools[0].flute_length, 1.);
            assert_eq!(restored.tools[0].corner_radius, Some(radius));
            assert_eq!(plan_setup(&restored, 1).unwrap().commands, program.commands);
            assert_adaptive_nc_roundtrip(restored);
        }
    }

    #[test]
    fn face_roughing_avoids_fallback_grid_for_small_radial_engagement() {
        let mut doc = face_fixture();
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        parameters.optimal_load = 0.8;
        assert!(plan_setup(&doc, 1).unwrap().commands.len() < 2000);
    }

    #[test]
    fn face_roughing_replays_nc_in_mm_and_inches_and_roundtrips_ap() {
        let mut doc = face_fixture();

        doc.tools[0].corner_radius = Some(1.5);
        doc.tools[0].flute_length = 2.5;
        let saved = serde_json::to_string(&doc).unwrap();
        let loaded: CamDocumentDto = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.tools[0].maximum_axial_depth, Some(1.0));
        assert_eq!(
            plan_setup(&loaded, 1).unwrap(),
            plan_setup(&doc, 1).unwrap()
        );
        assert_adaptive_nc_roundtrip(doc.clone());
        doc.units = CamUnits::Inches;
        assert_adaptive_nc_roundtrip(doc);
    }

    fn cylinder(center: Point2Dto, radius: f64, bottom: f64, top: f64) -> CamStockMeshDto {
        let mut mesh = CamStockMeshDto {
            positions: vec![center.x, center.y, bottom, center.x, center.y, top],
            indices: vec![],
        };
        const N: u32 = 96;
        for i in 0..N {
            let a = TAU * i as f64 / N as f64;
            for z in [bottom, top] {
                mesh.positions.extend([
                    center.x + radius * a.cos(),
                    center.y + radius * a.sin(),
                    z,
                ]);
            }
        }
        for i in 0..N {
            let a = 2 + 2 * i;
            let b = 2 + 2 * ((i + 1) % N);
            mesh.indices
                .extend([0, b, a, 1, a + 1, b + 1, a, b, b + 1, a, b + 1, a + 1]);
        }
        mesh
    }

    #[test]
    fn cylinder_exterior_avoids_square_air_passes_and_keeps_leads_clear() {
        use crate::{simulate_setup, CamSimulationRequestDto, CamSimulationTargetDto};
        let center = Point2Dto::new(8.0, 7.0);
        let meshes = vec![cylinder(center, 6.5, -3.0, 0.0)];
        let mut doc = fixture(meshes.clone());
        doc.setups[0].stock_spec = CamStockSpecDto::FromModel {
            shape: CamStockShape::Cylinder,
            offsets: CamStockOffsetsDto::default(),
        };
        doc.setups[0].resolved_stock = CamResolvedStockDto::Cylinder {
            center,
            radius: 7.0,
        };
        let mut link = crate::CamLinkingDto {
            operation_id: 1,
            ..Default::default()
        };
        link.lead_in.horizontal_radius = 0.2;
        link.lead_in.vertical_radius = 0.2;
        link.lead_in.linear_distance = 0.0;
        link.minimum_helix_diameter = 1.6;
        doc.linking.push(link);
        let program = plan_setup(&doc, 1).unwrap();
        assert!(
            program.warnings[0].contains("2 continuous exterior passes"),
            "{:?}",
            program.warnings
        );
        assert!(
            program.stats.cutting_distance < 180.0,
            "{:?}",
            program.stats
        );
        let sim = simulate_setup(
            &doc,
            &CamSimulationRequestDto {
                setup_id: 1,
                voxel_size: Some(0.2),
                max_voxels: None,
                stock_mesh: None,
                target: Some(CamSimulationTargetDto {
                    cache_key: None,
                    meshes,
                    tolerance_mm: 0.05,
                }),
                through_operation_id: None,
                completed_steps: None,
                playback_time_seconds: None,
            },
        )
        .unwrap();
        assert!(sim.removed_volume_mm3 > 8.0);
        assert!(sim.collisions.is_empty(), "{:?}", sim.collisions);
        assert_eq!(sim.comparison.unwrap().gouged_voxels, 0);
    }

    #[test]
    fn coarse_stepdown_clears_cap_and_shoulder_with_bull_nose_tool() {
        use crate::{simulate_setup, CamSimulationRequestDto, CamSimulationTargetDto};
        let center = Point2Dto::new(8.0, 7.0);
        let meshes = vec![
            cylinder(center, 6.0, -3.0, -1.5),
            cylinder(center, 3.5, -1.5, -0.5),
        ];
        let mut doc = fixture(meshes.clone());
        doc.setups[0].stock_spec = CamStockSpecDto::FromModel {
            shape: CamStockShape::Cylinder,
            offsets: CamStockOffsetsDto::default(),
        };
        doc.setups[0].resolved_stock = CamResolvedStockDto::Cylinder {
            center,
            radius: 6.5,
        };
        doc.tools[0].kind = CamToolKind::BullNoseEndMill;
        doc.tools[0].corner_radius = Some(0.3);
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        parameters.maximum_stepdown = 3.0;
        parameters.machine_cavities = false;
        let program = plan_setup(&doc, 1).unwrap();
        assert!(point_is_cut_at_depth(&program, center, -0.4, 1.7), "cap");
        assert!(
            point_is_cut_at_depth(&program, Point2Dto::new(13.0, 7.0), -1.4, 1.7),
            "shoulder"
        );
        let sim = simulate_setup(
            &doc,
            &CamSimulationRequestDto {
                setup_id: 1,
                voxel_size: Some(0.15),
                max_voxels: None,
                stock_mesh: None,
                target: Some(CamSimulationTargetDto {
                    cache_key: None,
                    meshes,
                    tolerance_mm: 0.05,
                }),
                through_operation_id: None,
                completed_steps: None,
                playback_time_seconds: None,
            },
        )
        .unwrap();
        assert!(sim.removed_volume_mm3 > 110.0, "{}", sim.removed_volume_mm3);
        assert!(sim.collisions.is_empty(), "{:?}", sim.collisions);
        assert_eq!(sim.comparison.unwrap().gouged_voxels, 0);
    }

    #[test]
    fn adaptive_boss_passes_are_rounded_clear_and_deterministic() {
        let doc = fixture(vec![cuboid([6.0, 5.0, -3.0], [10.0, 9.0, 0.0])]);
        let program = plan_setup(&doc, 1).unwrap();
        assert_eq!(program, super::super::plan_setup_uncached(&doc, 1).unwrap());
        assert_continuous_box_clearance(
            &program,
            Point2Dto::new(6.0, 5.0),
            Point2Dto::new(10.0, 9.0),
            0.0,
            2.1,
        );
        let mut previous = None;
        let mut arcs = 0;
        for command in &program.commands {
            match command {
                CamCommandDto::Circular {
                    to,
                    center,
                    clockwise,
                    ..
                } => {
                    let from: Point3Dto = previous.unwrap();
                    assert!(*clockwise, "external M3 climb direction");
                    let radius = dist(xy(from), xy(*center));
                    assert!(radius >= 0.8);
                    assert!((dist(xy(*to), xy(*center)) - radius).abs() < 1e-7);
                    previous = Some(*to);
                    arcs += 1;
                }
                CamCommandDto::Linear { to, .. } | CamCommandDto::Rapid { to } => {
                    previous = Some(*to);
                }
                _ => {}
            }
        }
        assert!(
            arcs > 10 && arcs < 200,
            "excessive redundant circle count: {arcs}"
        );
        assert!(program
            .warnings
            .iter()
            .any(|w| w.contains("sampled maximum")));
    }

    #[test]
    fn continuous_exterior_covers_rotated_convex_stock_bands() {
        for angle in [0.0_f64, 0.23, 0.71] {
            let mut mesh = cuboid([4.0, 4.0, -3.0], [12.0, 10.0, 0.0]);
            for vertex in mesh.positions.as_chunks_mut::<3>().0 {
                let (x, y) = (vertex[0] - 8.0, vertex[1] - 7.0);
                vertex[0] = 8.0 + x * angle.cos() - y * angle.sin();
                vertex[1] = 7.0 + x * angle.sin() + y * angle.cos();
            }
            let doc = fixture(vec![mesh.clone()]);
            let setup = &doc.setups[0];
            let program = plan_setup(&doc, 1).unwrap();
            let polygon = mesh
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .take(4)
                .map(|v| Point2Dto::new(v[0], v[1]))
                .collect::<Vec<_>>();
            assert_continuous_polygon_clearance(&program, &polygon, 0.0, 2.1);
            let e = Envelope::new(setup, 0.2, 6.2).unwrap();
            let mut e = Envelope {
                target: vec![f64::NEG_INFINITY; e.target.len()],
                ..e
            };
            let mut target = std::mem::take(&mut e.target);
            e.rasterize(&mesh, setup, &mut target, &mut Work::default())
                .unwrap();
            e.target = target;
            let front = ConvexStock::from_envelope(&e, -1.0, 0.1, 0.1, &mut Work::default())
                .unwrap()
                .unwrap();
            let mut checked = 0;
            for y in 0..70 {
                for x in 0..80 {
                    let p = Point2Dto::new((x as f64 + 0.5) * 0.2, (y as f64 + 0.5) * 0.2);
                    if front.distance(p) > front.offset + 1e-5 {
                        assert!(
                            point_is_cut_at_depth(&program, p, -1.0, 2.0),
                            "uncut convex exterior at {p:?}, angle {angle}"
                        );
                        checked += 1;
                    }
                }
            }
            assert!(checked > 2000);
            assert!(program.warnings[0].contains("0 fallback rounded laps"));
            let bound = [
                Point2Dto::new(0.0, 0.0),
                Point2Dto::new(16.0, 0.0),
                Point2Dto::new(16.0, 14.0),
                Point2Dto::new(0.0, 14.0),
            ]
            .into_iter()
            .map(|p| front.distance(p))
            .fold(0.0, f64::max);
            let passes = 2 * ((bound + 1e-4 - front.offset) / 1.0).ceil() as usize;
            assert!(
                program.commands.len() <= passes * (2 * front.vertices() + 12) + 16,
                "angle {angle}: {} commands, {} hull vertices; {:?}",
                program.commands.len(),
                front.vertices(),
                program.warnings
            );
        }
    }

    #[test]
    fn continuous_exterior_keeps_open_concavity_fallback_and_target_protection() {
        use crate::{simulate_setup, CamSimulationRequestDto, CamSimulationTargetDto};
        let meshes = vec![
            cuboid([2.0, 2.0, -3.0], [4.0, 12.0, 0.0]),
            cuboid([12.0, 2.0, -3.0], [14.0, 12.0, 0.0]),
            cuboid([4.0, 2.0, -3.0], [12.0, 4.0, 0.0]),
        ];
        let doc = fixture(meshes.clone());
        let program = plan_setup(&doc, 1).unwrap();
        assert!(program.warnings[0].contains("continuous exterior passes"));
        assert!(!program.warnings[0].contains("0 fallback rounded laps"));
        assert!(
            point_is_cut_at_depth(&program, Point2Dto::new(8.0, 8.0), -1.0, 2.0),
            "open concavity must remain accessible after exterior clearing"
        );
        let request = CamSimulationRequestDto {
            setup_id: 1,
            voxel_size: Some(0.4),
            max_voxels: None,
            stock_mesh: None,
            target: Some(CamSimulationTargetDto {
                cache_key: None,
                meshes,
                tolerance_mm: 0.1,
            }),
            through_operation_id: None,
            completed_steps: None,
            playback_time_seconds: None,
        };
        let simulation = simulate_setup(&doc, &request).unwrap();
        assert!(
            simulation.collisions.is_empty(),
            "{:?}",
            simulation.collisions
        );
        assert_eq!(simulation.comparison.unwrap().gouged_voxels, 0);
    }

    #[test]
    fn adaptive_enclosed_pocket_uses_pitch_limited_helix_and_bottom_lap() {
        let doc = cavity_fixture();
        let program = plan_setup(&doc, 1).unwrap();
        let mut previous = None;
        let mut helical_halves = 0;
        let mut bottom_laps = 0;
        for cmd in &program.commands {
            match cmd {
                CamCommandDto::Circular {
                    to, center, feed, ..
                } => {
                    let from: Point3Dto = previous.unwrap();
                    if to.z < from.z - EPS {
                        assert!(
                            (from.z - to.z) * 2.0
                                <= (TAU * 0.8 * 3.0_f64.to_radians().tan()).min(0.5) + EPS
                        );
                        assert_eq!(*feed, 100.0);
                        helical_halves += 1;
                    } else if (to.z + 1.0).abs() < EPS && *feed == 100.0 {
                        bottom_laps += 1;
                    }
                    assert!(center.x - 2.8 >= 2.1 - EPS && center.x + 2.8 <= 13.9 + EPS);
                    assert!(center.y - 2.8 >= 2.1 - EPS && center.y + 2.8 <= 11.9 + EPS);
                    previous = Some(*to);
                }
                CamCommandDto::Linear { to, .. } | CamCommandDto::Rapid { to } => {
                    previous = Some(*to);
                }
                _ => {}
            }
        }
        assert!(helical_halves > 2);
        assert_eq!(
            bottom_laps, 2,
            "helix must finish with one complete full-depth lap"
        );
    }

    #[test]
    fn adaptive_envelope_covers_sloped_triangles_and_is_depth_dependent() {
        let doc = fixture(vec![]);
        let setup = &doc.setups[0];
        let mut e = Envelope::new(setup, 0.2, 1.0).unwrap();
        let mesh = CamStockMeshDto {
            positions: vec![3.0, 3.0, -3.0, 9.0, 3.0, 0.0, 3.0, 9.0, -3.0],
            indices: vec![0, 1, 2],
        };
        let mut h = e.target.clone();
        e.rasterize(&mesh, setup, &mut h, &mut Work::default())
            .unwrap();
        e.target = h;
        let i = e.index(Point2Dto::new(5.1, 4.1)).unwrap();
        assert!(e.target[i] >= -1.9 - EPS && e.target[i] <= -1.8 + EPS);
        assert!(e.clearance(-1.0, 0.0)[i] > 0.0);
        assert_eq!(e.clearance(-2.0, 0.0)[i], 0.0);
    }

    #[test]
    fn adaptive_rejects_missing_malformed_and_over_budget_geometry() {
        let mut doc = fixture(vec![]);
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("current target geometry"));
        let CamOperationDto::Adaptive3d {
            geometry,
            parameters,
            ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        geometry.as_mut().unwrap().targets = vec![CamStockMeshDto {
            positions: vec![f64::NAN, 0.0, 0.0],
            indices: vec![0, 0, 0],
        }];
        parameters.tolerance = 0.00001;
        assert!(plan_setup(&doc, 1).unwrap_err().0.contains("grid exceeds"));
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        parameters.tolerance = 0.2;
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("finite indexed"));
    }

    #[test]
    fn adaptive_stay_down_requires_a_proven_clear_capsule() {
        let doc = fixture(vec![]);
        let setup = &doc.setups[0];
        let mut c = Cleared::new(3.0, xy(setup.stock.min));
        c.add(Point2Dto::new(5.0, 5.0));
        c.add(Point2Dto::new(11.0, 5.0));
        assert!(link_clear(
            setup,
            &c,
            Point2Dto::new(4.5, 5.0),
            Point2Dto::new(5.5, 5.0),
            2.0
        ));
        assert!(!link_clear(
            setup,
            &c,
            Point2Dto::new(5.0, 5.0),
            Point2Dto::new(11.0, 5.0),
            2.0
        ));
        assert!(!cutter_clear(setup, &c, Point2Dto::new(8.0, 5.0), 2.0));
    }

    #[test]
    fn adaptive_engagement_sums_disjoint_sectors_and_pads_each_boundary() {
        assert_eq!(engagement_angle(&[false; 128]), 0.0);
        assert_eq!(engagement_angle(&[true; 128]), TAU);
        let mut ring = [false; 128];
        ring[0..8].fill(true);
        ring[64..72].fill(true);
        assert!((engagement_angle(&ring) - 20.0 * TAU / 128.0).abs() < EPS);

        ring.fill(false);
        ring[124..128].fill(true);
        ring[0..4].fill(true);
        assert!((engagement_angle(&ring) - 10.0 * TAU / 128.0).abs() < EPS);
    }

    #[test]
    fn adaptive_analytic_engagement_bounds_independent_dense_samples() {
        let doc = fixture(vec![]);
        for stock in [
            CamResolvedStockDto::Box,
            CamResolvedStockDto::Cylinder {
                center: Point2Dto::new(8.0, 7.0),
                radius: 5.0,
            },
            CamResolvedStockDto::Hex {
                center: Point2Dto::new(8.0, 7.0),
                across_flats: 10.0,
            },
        ] {
            let mut setup = doc.setups[0].clone();
            setup.resolved_stock = stock;
            let envelope = Envelope::new(&setup, 0.2, 6.0).unwrap();
            let mut cleared = Cleared::new(2.8, xy(setup.stock.min));
            cleared.add(Point2Dto::new(5.0, 7.0));
            cleared.add(Point2Dto::new(8.0, 7.0));
            for x in [-1.0, 1.0, 4.0, 6.0, 8.0, 11.0, 15.0] {
                for y in [0.0, 2.0, 6.0, 9.0, 13.0] {
                    let c = Point2Dto::new(x, y);
                    let upper =
                        analytic_engagement(&setup, &cleared, c, 2.0, 0.0, &mut Work::default())
                            .unwrap();
                    let count = (0..4096)
                        .filter(|&i| {
                            let p = polar(c, 2.0, TAU * (i as f64 + 0.5) / 4096.0);
                            envelope.initially_occupied(&setup, p, -1.0)
                                && cleared.centers.iter().all(|&cc| dist(cc, p) > 2.8)
                        })
                        .count();
                    assert!(
                        upper + 0.01 >= count as f64 * TAU / 4096.0,
                        "underestimated at {c:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn adaptive_empty_disk_certificate_checks_interior_not_just_boundary() {
        let doc = fixture(vec![]);
        let setup = &doc.setups[0];
        let center = Point2Dto::new(8.0, 7.0);
        let mut cleared = Cleared::new(1.0, xy(setup.stock.min));
        for a in [0.0, PI * 0.5, PI, PI * 1.5] {
            cleared.add(polar(center, 1.1, a));
        }
        assert!((0..360).all(|i| cleared.contains(polar(center, 1.0, TAU * i as f64 / 360.0))));
        assert!(!cleared.contains(center));
        assert!(!disk_already_clear(setup, &cleared, center, 1.0));
        cleared.add(center);
        assert!(disk_already_clear(setup, &cleared, center, 0.8));
    }

    #[test]
    fn adaptive_nc_roundtrip_preserves_helices_and_target_stock() {
        assert_adaptive_nc_roundtrip(cavity_fixture());
    }

    #[test]
    fn adaptive_nc_roundtrip_preserves_continuous_exterior_arcs_mm_and_inches() {
        let mut mesh = cuboid([4.0, 4.0, -3.0], [12.0, 10.0, 0.0]);
        let angle = 0.23_f64;
        for vertex in mesh.positions.as_chunks_mut::<3>().0 {
            let (x, y) = (vertex[0] - 8.0, vertex[1] - 7.0);
            vertex[0] = 8.0 + x * angle.cos() - y * angle.sin();
            vertex[1] = 7.0 + x * angle.sin() + y * angle.cos();
        }
        let mut doc = fixture(vec![mesh]);
        assert_adaptive_nc_roundtrip(doc.clone());
        doc.units = CamUnits::Inches;
        assert_adaptive_nc_roundtrip(doc);
    }

    fn assert_adaptive_nc_roundtrip(mut doc: CamDocumentDto) {
        crate::post::tests::bind_test_names(&mut doc, &[(1, "AdaptiveTool")]);
        doc.tools[0].number = Some(1);
        use crate::{
            post::post_setup_unchecked as post_setup, simulate_gcode, simulate_setup,
            CamGcodeDialectDto, CamGcodeSimulationRequestDto, CamPostRequestDto,
            CamSimulationRequestDto, CamSimulationStepKind, CamSimulationTargetDto,
        };
        let CamOperationDto::Adaptive3d {
            geometry: Some(geometry),
            ..
        } = &doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        let target = Some(CamSimulationTargetDto {
            cache_key: None,
            meshes: geometry.targets.clone(),
            tolerance_mm: 0.0,
        });
        let predicted = simulate_setup(
            &doc,
            &CamSimulationRequestDto {
                setup_id: 1,
                voxel_size: Some(0.5),
                max_voxels: None,
                stock_mesh: None,
                target: target.clone(),
                through_operation_id: None,
                completed_steps: None,
                playback_time_seconds: None,
            },
        )
        .unwrap();
        assert!(predicted.removed_voxels > 100);
        assert_eq!(predicted.comparison.as_ref().unwrap().gouged_voxels, 0);
        assert!(
            predicted.collisions.is_empty(),
            "{:?}",
            predicted.collisions
        );
        for (post_dialect, nc_dialect) in [
            (PostDialect::Siemens828d, CamGcodeDialectDto::Siemens828d),
            (PostDialect::Fanuc, CamGcodeDialectDto::Fanuc),
        ] {
            let posted = post_setup(
                &doc,
                &CamPostRequestDto {
                    setup_id: 1,
                    program_name: Some("ADAPTIVE_TEST".into()),
                    post: Some(CamPostConfigDto {
                        machine_retract_z: Some(0.0),
                        tool_call_mode: crate::CamToolCallMode::Automatic,
                        dialect: post_dialect,
                        program_number: Some(123),
                        sequence_numbers: false,
                        siemens_828d: (post_dialect == PostDialect::Siemens828d)
                            .then(Siemens828dPostConfigDto::default),
                    }),
                },
            )
            .unwrap();
            let nc = simulate_gcode(
                &doc,
                &CamGcodeSimulationRequestDto {
                    setup_id: 1,
                    source: posted.nc,
                    file_name: None,
                    dialect: nc_dialect,
                    voxel_size: Some(0.5),
                    max_voxels: None,
                    stock_mesh: None,
                    target: target.clone(),
                    completed_steps: None,
                },
            )
            .unwrap();
            assert_eq!(nc.comparison.as_ref().unwrap().gouged_voxels, 0);
            assert!(nc.collisions.is_empty(), "{:?}", nc.collisions);
            assert!(
                predicted.removed_voxels.abs_diff(nc.removed_voxels) <= 4,
                "CAM/NC removal mismatch"
            );
            let a = predicted
                .steps
                .iter()
                .filter(|s| s.kind == CamSimulationStepKind::Circular)
                .collect::<Vec<_>>();
            let b = nc
                .steps
                .iter()
                .filter(|s| s.kind == CamSimulationStepKind::Circular)
                .collect::<Vec<_>>();
            assert_eq!(a.len(), b.len());
            for (a, b) in a.into_iter().zip(b) {
                assert_eq!(a.clockwise, b.clockwise);
                assert_eq!(a.plane, b.plane);
                let (a_to, b_to) = (a.to.unwrap(), b.to.unwrap());
                assert!(dist(xy(a_to), xy(b_to)) < 0.002 && (a_to.z - b_to.z).abs() < 0.001);
                assert!((a.duration_seconds - b.duration_seconds).abs() < 0.01);
            }
        }
    }

    #[test]
    fn adaptive_cache_tracks_geometry_parameters_and_tool_changes() {
        let mut doc = cavity_fixture();
        let first = plan_setup(&doc, 1).unwrap();
        assert_eq!(first, plan_setup(&doc, 1).unwrap());
        let CamOperationDto::Adaptive3d { cutting, .. } = &mut doc.setups[0].operations[0] else {
            unreachable!()
        };
        cutting.feed_xy *= 0.5;
        let slower = plan_setup(&doc, 1).unwrap();
        assert!(slower.stats.estimated_seconds > first.stats.estimated_seconds);
        doc.tools[0].center_cutting = false;
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("center-cutting"));
        doc.tools[0].center_cutting = true;
        let CamOperationDto::Adaptive3d { geometry, .. } = &mut doc.setups[0].operations[0] else {
            unreachable!()
        };
        geometry.as_mut().unwrap().targets = vec![cuboid([0.0, 0.0, -3.0], [16.0, 14.0, 0.0])];
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("no accessible cutting area"));
    }

    #[test]
    fn adaptive_cavities_and_unsupported_stock_are_explicit() {
        let mut doc = cavity_fixture();
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        parameters.machine_cavities = false;
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("no accessible cutting area"));
        doc.setups[0].resolved_stock = CamResolvedStockDto::ModelBody { body_id: 1 };
        doc.setups[0].stock_spec = CamStockSpecDto::ModelBody { body_id: 1 };
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("modeled stock mesh is missing"));
    }

    #[test]
    fn terrace_schedule_restarts_stepdown_without_redundant_layers() {
        let meshes = vec![
            cuboid([4.0, 4.0, -3.0], [12.0, 10.0, -1.4]),
            cuboid([6.0, 5.0, -1.4], [10.0, 9.0, -0.3]),
        ];
        let doc = fixture(meshes.clone());
        let CamOperationDto::Adaptive3d { parameters, .. } = &doc.setups[0].operations[0] else {
            unreachable!()
        };
        let mut p = parameters.clone();
        p.maximum_stepdown = 0.8;
        let levels =
            roughing_depth_levels(&doc.setups[0], &meshes, 0.0, -2.0, f64::INFINITY, &p).unwrap();
        let expected = [-0.2, -1.0, -1.3, -2.0];
        assert_eq!(levels.len(), expected.len());
        for (&a, b) in levels.iter().zip(expected) {
            assert!((a - b).abs() < EPS);
        }
        let mut previous = 0.0;
        for &level in &levels {
            assert!(previous > level && previous - level <= p.maximum_stepdown + EPS);
            previous = level;
        }

        p.maximum_stepdown = 22.5;
        let deep =
            roughing_depth_levels(&doc.setups[0], &meshes, 0.0, -2.0, f64::INFINITY, &p).unwrap();
        assert_eq!(deep.len(), 3);
        for (&a, b) in deep.iter().zip([-0.2, -1.3, -2.0]) {
            assert!((a - b).abs() < EPS);
        }
        assert_eq!(
            roughing_depth_levels(&doc.setups[0], &[], 0.0, -2.0, f64::INFINITY, &p).unwrap(),
            vec![-2.0]
        );
    }

    #[test]
    fn adaptive_top_preserves_stepdown_levels_and_adds_accessible_terraces() {
        let mut doc = fixture(vec![cuboid([6.0, 5.0, -3.0], [10.0, 9.0, 0.0])]);
        let CamOperationDto::Adaptive3d { top_z, .. } = &mut doc.setups[0].operations[0] else {
            unreachable!()
        };
        *top_z = 0.5;
        let program = plan_setup(&doc, 1).unwrap();
        let mut levels = program
            .commands
            .iter()
            .filter_map(|c| match c {
                CamCommandDto::Circular { to, .. } => Some(to.z),
                _ => None,
            })
            .collect::<Vec<_>>();
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        assert_eq!(levels, vec![-2.0, -1.5, -0.5]);
        let CamOperationDto::Adaptive3d {
            top_z, parameters, ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        *top_z = -0.5;
        parameters.maximum_stepdown = 3.0;
        assert!(plan_setup(&doc, 1).is_ok());
        let encoded = serde_json::to_string(&doc).unwrap();
        let decoded: CamDocumentDto = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            plan_setup(&doc, 1).unwrap(),
            plan_setup(&decoded, 1).unwrap()
        );
    }

    #[test]
    fn adaptive_lower_top_adds_bands_without_certifying_removed_stock() {
        let mut doc = fixture(vec![cuboid([6.0, 5.0, -3.0], [10.0, 9.0, 0.0])]);
        let CamOperationDto::Adaptive3d {
            top_z, parameters, ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        *top_z = -0.5;
        parameters.maximum_stepdown = 3.0;
        doc.tools[0].flute_length = 1.75;
        assert!(plan_setup(&doc, 1).is_ok());
        assert_eq!(doc.tools[0].flute_length, 1.75);
        doc.tools[0].flute_length = 10.0;
        let CamOperationDto::Adaptive3d { feed_height_z, .. } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        *feed_height_z = -0.25;
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("known incoming stock top"));
        let CamOperationDto::Adaptive3d {
            feed_height_z,
            parameters,
            ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        *feed_height_z = 1.0;
        parameters.maximum_stepdown = 1.0;
        assert!(plan_setup(&doc, 1).is_ok());
        assert_adaptive_nc_roundtrip(doc);
    }

    #[test]
    fn adaptive_lower_top_accepts_proved_facing_but_not_partial_or_suppressed_facing() {
        let mut doc = fixture(vec![cuboid([6.0, 5.0, -3.0], [10.0, 9.0, -0.5])]);
        let CamOperationDto::Adaptive3d {
            top_z,
            feed_height_z,
            ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        *top_z = -0.5;
        *feed_height_z = -0.25;
        doc.setups[0].operations.insert(
            0,
            CamOperationDto::Face {
                id: 2,
                name: "Whole stock face".into(),
                enabled: true,
                tool_id: 1,
                bounds: Rect2Dto {
                    min: Point2Dto::new(0.0, 0.0),
                    max: Point2Dto::new(16.0, 14.0),
                },
                top_z: 0.0,
                target_z: -0.5,
                step_over: 2.0,
                step_down: 0.5,
                safe_distance: 2.0,
                direction: FaceDirection::BothWays,
                clearance_z: 5.0,
                retract_z: 3.0,
                feed_height_z: 1.0,
                cutting: doc.tools[0].cutting,
            },
        );
        doc.next_operation_id = 3;
        assert!(plan_setup(&doc, 1).is_ok());
        let CamOperationDto::Face { enabled, .. } = &mut doc.setups[0].operations[0] else {
            unreachable!()
        };
        *enabled = false;
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("known incoming stock top"));
        let CamOperationDto::Face {
            enabled, bounds, ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        *enabled = true;
        bounds.min.y = 5.0;
        bounds.max.y = 9.0;
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("known incoming stock top"));
    }

    #[test]
    fn adaptive_traverse_plane_must_clear_a_target_above_stock() {
        let doc = fixture(vec![cuboid([6.0, 5.0, -3.0], [10.0, 9.0, 10.0])]);
        assert!(plan_setup(&doc, 1)
            .unwrap_err()
            .0
            .contains("above all target geometry"));
    }
}
