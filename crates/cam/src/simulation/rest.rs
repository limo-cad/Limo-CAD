//! Remaining material is a volume in model space, independent of clamping.
//! Transfer whole occupied cells, never just their centers: tilted thin walls
//! must not disappear between grids. SAT excludes cells that merely touch.
use super::*;

fn xyz(p: Point3Dto) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(super) fn transfer(
    source: VoxelStock,
    from: WorkCoordinateSystemDto,
    to: WorkCoordinateSystemDto,
    spec: &GridSpec,
    cancellation: Option<&CamSimulationCancellation>,
) -> Result<VoxelStock, CamPlanError> {
    if from == to
        && source.min == spec.min
        && source.dimensions == spec.dimensions
        && source.cell_size == spec.cell_size
    {
        return Ok(source);
    }
    let source_axes = [from.x_axis, from.y_axis, from.z_axis];
    let axes = [to.x_axis, to.y_axis, to.z_axis].map(|a| source_axes.map(|b| dot(a, b)));
    let half = spec.cell_size.map(|s| s * 0.5);
    let source_half = source.cell_size.map(|s| s * 0.5);
    let extent: [f64; 3] =
        std::array::from_fn(|i| (0..3).map(|j| axes[j][i].abs() * half[j]).sum());
    let unit = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];

    let mut tests = Vec::new();
    for axis in unit
        .into_iter()
        .chain(axes)
        .chain(unit.into_iter().flat_map(|a| axes.map(|b| cross(a, b))))
    {
        let norm = dot(axis, axis).sqrt();
        if norm < 1e-10 {
            continue;
        }
        let a = axis.map(|v| v / norm);
        let radius = (0..3)
            .map(|i| a[i].abs() * source_half[i] + dot(a, axes[i]).abs() * half[i])
            .sum::<f64>();
        tests.push((a, radius));
    }
    let source_min = xyz(source.min);
    let mut result = VoxelStock::filled(spec, |_| false);

    result.display_cuts.limited = true;
    result.display_cuts.reoriented = true;
    result.mesh_quality_warnings = source.mesh_quality_warnings.clone();
    let aligned = axes
        .iter()
        .all(|a| a.iter().filter(|v| v.abs() > 1e-8).count() == 1);
    let grid_exact = aligned && {
        let p = xyz(from.from_model(to.to_model(result.center(0, 0, 0))));
        (0..3).all(|i| {
            let j = (0..3).find(|&j| axes[j][i].abs() > 1e-8).unwrap();
            let cell = (p[i] - source_min[i]) / source.cell_size[i] - 0.5;
            (spec.cell_size[j] - source.cell_size[i]).abs() < 1e-9
                && (cell - cell.round()).abs() < 1e-6
        })
    };
    if !grid_exact {
        result.mesh_quality_warnings.push(format!("Remaining stock was conservatively transferred between {} grids at {:.3} mm detail. Boundary cells may retain up to one destination-cell diagonal of extra material.", if aligned { "offset" } else { "angled setups'" }, spec.cell_size.iter().copied().fold(0.,f64::max)));
    }

    if axes[2][2].abs() > 1. - 1e-8 {
        if let Some((center, radius)) = source
            .display_cuts
            .initial
            .as_ref()
            .and_then(|b| b.cylinder())
        {
            let p = to.from_model(from.to_model(Point3Dto::new(center.x, center.y, source.min.z)));
            result.display_cuts.initial = Some(surface::StockBoundary::new(
                spec,
                CamResolvedStockDto::Cylinder {
                    center: crate::Point2Dto::new(p.x, p.y),
                    radius,
                },
            ));
        }
    }
    let mut work = 0usize;
    for z in 0..spec.dimensions[2] {
        if let Some(c) = cancellation {
            c.check()?;
        }
        for y in 0..spec.dimensions[1] {
            for x in 0..spec.dimensions[0] {
                let p = xyz(from.from_model(to.to_model(result.center(x, y, z))));
                let center_cell = std::array::from_fn(|i| {
                    ((p[i] - source_min[i]) / source.cell_size[i]).floor() as isize
                });
                let mut occupied = source.occupied_at(center_cell);
                if !occupied {
                    let lo: [isize; 3] = std::array::from_fn(|i| {
                        (((p[i] - extent[i] - source_min[i]) / source.cell_size[i] + 1e-9).floor()
                            as isize)
                            .max(0)
                    });
                    let hi: [isize; 3] = std::array::from_fn(|i| {
                        (((p[i] + extent[i] - source_min[i]) / source.cell_size[i] - 1e-9).floor()
                            as isize)
                            .min(source.dimensions[i] as isize - 1)
                    });
                    'search: for sz in lo[2]..=hi[2] {
                        for sy in lo[1]..=hi[1] {
                            for sx in lo[0]..=hi[0] {
                                work += 1;
                                if work > 150_000_000 {
                                    return Err(CamPlanError("Angled remaining-stock transfer exceeded its work budget; use coarser simulation detail.".into()));
                                }
                                if !source.occupied_at([sx, sy, sz]) {
                                    continue;
                                }
                                let q = xyz(source.center(sx as usize, sy as usize, sz as usize));
                                let delta = std::array::from_fn(|i| p[i] - q[i]);
                                if tests
                                    .iter()
                                    .all(|(axis, radius)| dot(delta, *axis).abs() < radius - 1e-10)
                                {
                                    occupied = true;
                                    break 'search;
                                }
                            }
                        }
                    }
                }
                if occupied {
                    let i = result.index(x, y, z);
                    result.occupied[i / 64] |= 1 << (i % 64);
                    result.occupied_count += 1;
                }
            }
        }
    }
    Ok(result)
}

/// A conservative upper envelope of the full transferred volume. Voids below
/// overhangs are intentionally retained for fixed-axis engagement planning.
pub(crate) struct RestHeightMap {
    min: [f64; 2],
    cell: [f64; 2],
    dimensions: [usize; 2],
    heights: Vec<f64>,
}
impl RestHeightMap {
    #[cfg(test)]
    pub(crate) fn from_heights(
        min: [f64; 2],
        cell: [f64; 2],
        dimensions: [usize; 2],
        heights: Vec<f64>,
    ) -> Self {
        Self {
            min,
            cell,
            dimensions,
            heights,
        }
    }
    pub(crate) fn top(&self) -> f64 {
        self.heights
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
    }
    pub(crate) fn upper_over(&self, min: [f64; 2], max: [f64; 2]) -> f64 {
        let lo: [usize; 2] = std::array::from_fn(|i| {
            (((min[i] - self.min[i]) / self.cell[i]).floor() as isize)
                .clamp(0, self.dimensions[i] as isize) as usize
        });
        let hi: [usize; 2] = std::array::from_fn(|i| {
            (((max[i] - self.min[i]) / self.cell[i]).ceil() as isize)
                .clamp(0, self.dimensions[i] as isize) as usize
        });
        let mut top = f64::NEG_INFINITY;
        for y in lo[1]..hi[1] {
            for x in lo[0]..hi[0] {
                top = top.max(self.heights[x + self.dimensions[0] * y]);
            }
        }
        top
    }
}

pub(crate) fn planning_stock(
    document: &CamDocumentDto,
    setup: &CamSetupDto,
) -> Result<RestHeightMap, CamPlanError> {
    Ok(height_map(&planning_grid(document, setup)?, false))
}

/// Incoming stock of a setup at roughing's planning tolerance, including
/// remaining material transferred from a source setup.
fn planning_grid(
    document: &CamDocumentDto,
    setup: &CamSetupDto,
) -> Result<VoxelStock, CamPlanError> {
    let tolerance = setup
        .operations
        .iter()
        .filter_map(|o| match o {
            crate::CamOperationDto::Adaptive3d { parameters, .. } if o.enabled() => {
                Some(parameters.tolerance)
            }
            _ => None,
        })
        .fold(f64::INFINITY, f64::min);
    let mesh = setup.operations.iter().find_map(|o| match o {
        crate::CamOperationDto::Adaptive3d {
            geometry: Some(g), ..
        } => g.stock.as_ref(),
        _ => None,
    });
    let spec = GridSpec::for_stock(&setup.stock, Some(tolerance), HARD_MAX_VOXELS)?;
    initial_stock(document, setup, &spec, mesh, None)
}

/// What this setup's program so far (from its work offset) leaves of the
/// incoming stock, simulated at roughing's planning tolerance. Each call only
/// sweeps commands planned since the previous one.
pub(crate) struct PlanningStock {
    stock: VoxelStock,
    done: usize,
}

impl PlanningStock {
    pub(crate) fn new(
        document: &CamDocumentDto,
        setup: &CamSetupDto,
    ) -> Result<Self, CamPlanError> {
        Ok(Self {
            stock: planning_grid(document, setup)?,
            done: 0,
        })
    }

    /// Upper envelope after `commands`. Cells are emptied when their centers
    /// are cut, so a column may hide material up to a cell beside it: each
    /// column takes its neighbors' tops, never under-reporting stock.
    pub(crate) fn after(
        &mut self,
        document: &CamDocumentDto,
        setup: &CamSetupDto,
        commands: &[crate::CamCommandDto],
    ) -> Result<RestHeightMap, CamPlanError> {
        if commands.len() > self.done {
            let earlier = &commands[..self.done];
            let program = crate::CamProgramDto {
                setup_id: setup.id,
                name: setup.name.clone(),
                commands: commands.to_vec(),
                stats: Default::default(),
                per_operation: Vec::new(),
                work_offsets: setup.work_offsets(),
                warnings: Vec::new(),
            };
            run_program(
                document,
                &program,
                &mut self.stock,
                ProgramRunOptions {
                    collect: false,
                    completed_steps: None,
                    source_lines: &[],
                    verification: None,
                    checkpoints: None,
                    cancellation: None,
                    resume: Some(ProgramResumeState {
                        next_command_index: self.done,
                        position: earlier.iter().rev().find_map(|c| c.endpoint()),
                        active_tool_id: earlier.iter().rev().find_map(|c| match c {
                            crate::CamCommandDto::ToolChange { tool_id, .. } => Some(*tool_id),
                            _ => None,
                        }),
                        sweep_samples: 0,
                        outcome: ProgramRunOutcome::default(),
                    }),
                },
            )?;
            self.done = commands.len();
        }
        Ok(height_map(&self.stock, true))
    }
}

fn height_map(stock: &VoxelStock, dilate: bool) -> RestHeightMap {
    let [nx, ny, nz] = stock.dimensions;
    let mut heights = vec![f64::NEG_INFINITY; nx * ny];
    for y in 0..ny {
        for x in 0..nx {
            for z in (0..nz).rev() {
                if stock.is_occupied_index(stock.index(x, y, z)) {
                    heights[x + nx * y] = stock.min.z + (z + 1) as f64 * stock.cell_size[2];
                    break;
                }
            }
        }
    }
    if dilate {
        let column = heights.clone();
        for y in 0..ny {
            for x in 0..nx {
                for (dx, dy) in [
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let (u, v) = (x as isize + dx, y as isize + dy);
                    if u >= 0 && v >= 0 && (u as usize) < nx && (v as usize) < ny {
                        let i = x + nx * y;
                        heights[i] = heights[i].max(column[u as usize + nx * v as usize]);
                    }
                }
            }
        }
    }
    RestHeightMap {
        min: [stock.min.x, stock.min.y],
        cell: [stock.cell_size[0], stock.cell_size[1]],
        dimensions: [nx, ny],
        heights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(roll: f64, pitch: f64, yaw: f64) -> WorkCoordinateSystemDto {
        let (sa, ca) = roll.to_radians().sin_cos();
        let (sb, cb) = pitch.to_radians().sin_cos();
        let (sc, cc) = yaw.to_radians().sin_cos();
        WorkCoordinateSystemDto {
            origin: Point3Dto::new(3., -7., 2.),
            x_axis: [cc * cb, sc * cb, -sb],
            y_axis: [cc * sb * sa - sc * ca, sc * sb * sa + cc * ca, cb * sa],
            z_axis: [cc * sb * ca + sc * sa, sc * sb * ca - cc * sa, cb * ca],
        }
    }
    fn source_spec() -> GridSpec {
        GridSpec::for_stock(
            &StockBoxDto {
                min: Point3Dto::new(-6., -5., -4.),
                max: Point3Dto::new(6., 5., 4.),
            },
            Some(1.),
            HARD_MAX_VOXELS,
        )
        .unwrap()
    }
    fn envelope(spec: &GridSpec) -> StockBoxDto {
        StockBoxDto {
            min: spec.min,
            max: Point3Dto::new(
                spec.min.x + spec.dimensions[0] as f64 * spec.cell_size[0],
                spec.min.y + spec.dimensions[1] as f64 * spec.cell_size[1],
                spec.min.z + spec.dimensions[2] as f64 * spec.cell_size[2],
            ),
        }
    }
    fn at(stock: &VoxelStock, p: Point3Dto) -> bool {
        let q = xyz(p);
        let min = xyz(stock.min);
        stock.occupied_at(std::array::from_fn(|i| {
            ((q[i] - min[i]) / stock.cell_size[i]).floor() as isize
        }))
    }
    #[test]
    fn all_six_clamping_directions_preserve_removal_exactly() {
        let spec = source_spec();
        let from = WorkCoordinateSystemDto::default();
        for (roll, pitch, yaw) in [
            (0., 0., 0.),
            (180., 0., 0.),
            (90., 0., 90.),
            (-90., 0., 0.),
            (0., 90., 0.),
            (0., -90., 180.),
        ] {
            let to = frame(roll, pitch, yaw);
            let dest = GridSpec::for_stock(
                &to.stock_from(from, &envelope(&spec)),
                Some(1.),
                HARD_MAX_VOXELS,
            )
            .unwrap();
            let stock = VoxelStock::filled(&spec, |p| {
                p.x * p.x + p.y * p.y > 9. && !(p.x > 2. && p.z > 0.)
            });
            let count = stock.occupied_count;
            let moved = transfer(stock, from, to, &dest, None).unwrap();
            assert_eq!(count, moved.occupied_count, "{roll} {pitch} {yaw}");
            for z in 0..spec.dimensions[2] {
                for y in 0..spec.dimensions[1] {
                    for x in 0..spec.dimensions[0] {
                        let p = Point3Dto::new(
                            spec.min.x + x as f64 + 0.5,
                            spec.min.y + y as f64 + 0.5,
                            spec.min.z + z as f64 + 0.5,
                        );
                        assert_eq!(
                            at(&moved, to.from_model(from.to_model(p))),
                            p.x * p.x + p.y * p.y > 9. && !(p.x > 2. && p.z > 0.)
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn oblique_transfer_keeps_thin_walls_and_disconnected_cells_without_filling_holes() {
        let spec = source_spec();
        let from = frame(11., -23., 7.);
        let to = frame(-37., 28., 51.);
        let dest = GridSpec::for_stock(
            &to.stock_from(from, &envelope(&spec)),
            Some(0.7),
            HARD_MAX_VOXELS,
        )
        .unwrap();
        let source = VoxelStock::filled(&spec, |p| {
            p.x.abs() > 5. || (p.x > 2. && p.y > 3. && p.z > 2.)
        });
        let moved = transfer(source.clone(), from, to, &dest, None).unwrap();

        for z in 0..spec.dimensions[2] {
            for y in 0..spec.dimensions[1] {
                for x in 0..spec.dimensions[0] {
                    if !source.is_occupied_index(source.index(x, y, z)) {
                        continue;
                    }
                    let center = source.center(x, y, z);
                    for dz in [-0.499, 0., 0.499] {
                        for dy in [-0.499, 0., 0.499] {
                            for dx in [-0.499, 0., 0.499] {
                                let p = Point3Dto::new(center.x + dx, center.y + dy, center.z + dz);
                                assert!(at(&moved, to.from_model(from.to_model(p))));
                            }
                        }
                    }
                }
            }
        }
        assert!(!at(
            &moved,
            to.from_model(from.to_model(Point3Dto::new(0., 0., 0.)))
        ));
        assert!(moved.occupied_count < dest.dimensions.iter().product::<usize>() / 2);
        assert!(moved
            .mesh_quality_warnings
            .iter()
            .any(|s| s.contains("conservatively transferred")));
    }
    #[test]
    fn empty_stock_stays_empty_after_tilt() {
        let spec = source_spec();
        let from = WorkCoordinateSystemDto::default();
        let to = frame(19., 37., 43.);
        let dest = GridSpec::for_stock(
            &to.stock_from(from, &envelope(&spec)),
            Some(1.),
            HARD_MAX_VOXELS,
        )
        .unwrap();
        assert_eq!(
            transfer(VoxelStock::filled(&spec, |_| false), from, to, &dest, None)
                .unwrap()
                .occupied_count,
            0
        );
    }
}
