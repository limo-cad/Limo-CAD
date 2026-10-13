//! Playback consumes the already-compensated physical timeline. One bounded
//! forward checkpoint avoids replaying an operation's earlier moves per frame.
use super::*;
mod gcode;

/// One prepared playback session. The desktop host owns this on a background
/// worker: no document cloning, cache-key serialization, target work or full
/// timeline transfer takes place on the frame clock. Only three stock bitsets
/// (scope start, completed moves, last display) are retained, plus one temporary
/// partial sweep. Ready meshes are separately byte-bounded by the host.
pub struct CamPlayback {
    document: CamDocumentDto,
    timeline: Vec<CamSimulationStepDto>,
    metadata: CamSimulationResultDto,
    base: VoxelStock,
    base_completed: usize,
    stock: VoxelStock,
    completed: usize,
    last_display: Option<VoxelStock>,
    last_display_warnings: Vec<String>,
    /// Extraction tiles of `last_display`, reused away from new cuts.
    tiles: Option<SurfaceCache>,
    start: f64,
    end: f64,
}

impl CamPlayback {
    pub fn new(
        document: CamDocumentDto,
        mut request: CamSimulationRequestDto,
        start: f64,
        cancellation: Option<&CamSimulationCancellation>,
    ) -> Result<Self, CamPlanError> {
        if !start.is_finite() || start < 0.0 {
            return Err(CamPlanError("Invalid playback start time".into()));
        }
        request.playback_time_seconds = None;
        request.completed_steps = None;
        let mut metadata = simulate_setup_with_cancellation(&document, &request, cancellation)?;
        let end = metadata.estimated_seconds;
        let start = start.min(end);
        let timeline = std::mem::take(&mut metadata.steps);

        metadata.stock_mesh = None;
        metadata.comparison = None;
        metadata.collisions.clear();
        metadata
            .warnings
            .retain(|warning| !warning.starts_with("Remaining-stock display uses a complete "));
        let setup = document
            .setup(request.setup_id)
            .ok_or_else(|| CamPlanError("CAM setup no longer exists".into()))?;
        let spec = GridSpec::for_stock(
            &setup.stock,
            request.voxel_size,
            request
                .max_voxels
                .unwrap_or(DEFAULT_MAX_VOXELS)
                .clamp(1, HARD_MAX_VOXELS),
        )?;
        let prepared = resolve_stage_cache(&document, setup, &spec, &request);
        let completed = timeline.partition_point(|step| step.cumulative_seconds <= start + EPSILON);
        let boundary = prepared.as_ref().and_then(|entry| {
            entry
                .checkpoints
                .iter()
                .rev()
                .find(|point| point.outcome.steps.len() <= completed)
        });
        let (base_completed, base) = if let Some(point) = boundary {
            (point.outcome.steps.len(), point.stock.clone())
        } else if let Some(entry) = prepared {
            (0, entry.initial_stock.clone())
        } else {
            (
                0,
                initial_stock(
                    &document,
                    setup,
                    &spec,
                    request.stock_mesh.as_ref(),
                    cancellation,
                )?,
            )
        };
        Ok(Self {
            document,
            timeline,
            metadata,
            stock: base.clone(),
            base,
            base_completed,
            completed: base_completed,
            last_display: None,
            last_display_warnings: Vec::new(),
            tiles: None,
            start,
            end,
        })
    }

    /// `stock_mesh == None` means identical to this session's previous sample,
    /// not empty stock. The host reuses that retained surface. Sampling is safe
    /// in either direction; a partial move is never saved as completed stock.
    pub fn sample(
        &mut self,
        time: f64,
        cancellation: Option<&CamSimulationCancellation>,
    ) -> Result<CamSimulationResultDto, CamPlanError> {
        if !time.is_finite() || time < self.start - EPSILON {
            return Err(CamPlanError("Invalid playback sample time".into()));
        }
        if let Some(cancellation) = cancellation {
            cancellation.check()?;
        }
        let time = time.min(self.end);
        let completed = self
            .timeline
            .partition_point(|step| step.cumulative_seconds <= time + EPSILON);
        if completed < self.completed {
            self.stock = self.base.clone();
            self.completed = self.base_completed;
        }
        let mut samples = 0;
        for step in &self.timeline[self.completed..completed] {
            sweep_step(
                &self.document,
                &mut self.stock,
                step,
                1.0,
                &mut samples,
                cancellation,
            )?;
        }
        self.completed = completed;
        let mut display = self.stock.clone();
        if let Some(step) = self.timeline.get(completed) {
            let fraction = if step.duration_seconds > EPSILON {
                ((time - step.cumulative_seconds + step.duration_seconds) / step.duration_seconds)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            if fraction > 0.0 {
                sweep_step(
                    &self.document,
                    &mut display,
                    step,
                    fraction,
                    &mut samples,
                    cancellation,
                )?;
            }
        }
        let changed = self.last_display.as_ref().is_none_or(|old| {
            old.occupied != display.occupied || old.display_cuts != display.display_cuts
        });
        let mut result = self.metadata.clone();
        if changed {
            self.last_display_warnings.clear();
            result.stock_mesh = Some(display.presentation_mesh_reusing(
                MAX_SURFACE_TRIANGLES,
                &mut self.last_display_warnings,
                self.last_display.as_ref(),
                &mut self.tiles,
            )?);
            self.last_display = Some(display.clone());
        }
        result
            .warnings
            .extend(self.last_display_warnings.iter().cloned());
        if let Some(cancellation) = cancellation {
            cancellation.check()?;
        }
        result.remaining_voxels = display.occupied_count;
        result.removed_voxels = result.initial_voxels - result.remaining_voxels;
        let cell_volume = display.cell_size.iter().product::<f64>();
        result.remaining_volume_mm3 = result.remaining_voxels as f64 * cell_volume;
        result.removed_volume_mm3 = result.removed_voxels as f64 * cell_volume;
        result.completed_steps = Some(completed);
        result.estimated_seconds = time;
        Ok(result)
    }
}

struct FrameCheckpoint {
    key: Vec<u8>,
    completed: usize,
    stock: VoxelStock,
}
fn frames() -> &'static Mutex<VecDeque<FrameCheckpoint>> {
    static CACHE: OnceLock<Mutex<VecDeque<FrameCheckpoint>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub(super) fn simulate(
    document: &CamDocumentDto,
    request: &CamSimulationRequestDto,
    time: f64,
    cancellation: Option<&CamSimulationCancellation>,
) -> Result<CamSimulationResultDto, CamPlanError> {
    if !time.is_finite() || time < 0.0 {
        return Err(CamPlanError(
            "CAM playback time must be finite and nonnegative".into(),
        ));
    }
    let mut complete_request = request.clone();
    complete_request.playback_time_seconds = None;
    complete_request.completed_steps = None;
    let mut result = simulate_setup_with_cancellation(document, &complete_request, cancellation)?;
    let time = time.min(result.estimated_seconds);
    if time >= result.estimated_seconds {
        return Ok(result);
    }
    let key = cache::key(document, &complete_request)?;
    let completed = result
        .steps
        .partition_point(|step| step.cumulative_seconds <= time + EPSILON);
    let setup = document
        .setup(request.setup_id)
        .ok_or_else(|| CamPlanError("CAM setup no longer exists".into()))?;
    let spec = GridSpec::for_stock(
        &setup.stock,
        request.voxel_size,
        request
            .max_voxels
            .unwrap_or(DEFAULT_MAX_VOXELS)
            .clamp(1, HARD_MAX_VOXELS),
    )?;
    let prepared = resolve_stage_cache(document, setup, &spec, &complete_request);
    let boundary = prepared.as_ref().and_then(|entry| {
        entry
            .checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.outcome.steps.len() <= completed)
    });
    let mut checkpoint =
        boundary.map(|boundary| (boundary.outcome.steps.len(), boundary.stock.clone()));
    {
        let mut frames = frames()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = frames
            .iter()
            .position(|frame| frame.key == key && frame.completed <= completed)
        {
            let frame = frames.remove(index).expect("located frame");
            if checkpoint
                .as_ref()
                .is_none_or(|(count, _)| *count <= frame.completed)
            {
                checkpoint = Some((frame.completed, frame.stock.clone()));
            }
            frames.push_back(frame);
        }
    }
    let (start, mut stock) = match checkpoint {
        Some(checkpoint) => checkpoint,
        None => (
            0,
            if let Some(entry) = &prepared {
                entry.initial_stock.clone()
            } else {
                initial_stock(
                    document,
                    setup,
                    &spec,
                    request.stock_mesh.as_ref(),
                    cancellation,
                )?
            },
        ),
    };
    let mut samples = 0;
    for step in &result.steps[start..completed] {
        sweep_step(document, &mut stock, step, 1.0, &mut samples, cancellation)?;
    }
    if let Some(cancellation) = cancellation {
        cancellation.check()?;
    }
    {
        let mut frames = frames()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        frames.retain(|frame| frame.key != key);
        frames.push_back(FrameCheckpoint {
            key,
            completed,
            stock: stock.clone(),
        });
        while frames.len() > 4
            || frames
                .iter()
                .map(|frame| {
                    frame.key.len()
                        + frame.stock.occupied.len() * 8
                        + frame.stock.display_cuts.bytes()
                })
                .sum::<usize>()
                > 16 * 1024 * 1024
        {
            frames.pop_front();
        }
    }

    if let Some(step) = result.steps.get(completed) {
        let start_time = step.cumulative_seconds - step.duration_seconds;
        let fraction = if step.duration_seconds > EPSILON {
            ((time - start_time) / step.duration_seconds).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if fraction > 0.0 {
            sweep_step(
                document,
                &mut stock,
                step,
                fraction,
                &mut samples,
                cancellation,
            )?;
        }
    }
    result.stock_mesh = Some(stock.presentation_mesh(MAX_SURFACE_TRIANGLES, &mut result.warnings)?);
    if let Some(cancellation) = cancellation {
        cancellation.check()?;
    }
    result.remaining_voxels = stock.occupied_count;
    result.removed_voxels = result.initial_voxels - stock.occupied_count;
    let volume = stock.cell_size.iter().product::<f64>();
    result.remaining_volume_mm3 = result.remaining_voxels as f64 * volume;
    result.removed_volume_mm3 = result.removed_voxels as f64 * volume;
    result.steps.truncate(completed);
    result.estimated_seconds = time;
    result.completed_steps = Some(completed);

    result.comparison = None;
    result.collisions.retain(|collision| {
        result
            .steps
            .iter()
            .any(|step| step.command_index == collision.command_index)
    });
    Ok(result)
}

fn sweep_step(
    document: &CamDocumentDto,
    stock: &mut VoxelStock,
    step: &CamSimulationStepDto,
    fraction: f64,
    samples: &mut usize,
    cancellation: Option<&CamSimulationCancellation>,
) -> Result<(), CamPlanError> {
    if let Some(cancellation) = cancellation {
        cancellation.check()?;
    }
    if !matches!(
        step.kind,
        CamSimulationStepKind::Linear | CamSimulationStepKind::Circular
    ) {
        return Ok(());
    }
    let (Some(from), Some(to), Some(tool)) = (
        step.from,
        step.to,
        step.tool_id.and_then(|id| document.tool(id)),
    ) else {
        return Ok(());
    };
    if step.kind == CamSimulationStepKind::Circular {
        let center = step
            .center
            .ok_or_else(|| CamPlanError("CAM playback arc has no center".into()))?;
        let mut arc = ArcSweep::new(
            from,
            center,
            to,
            step.clockwise.unwrap_or(false),
            step.plane.unwrap_or(CamArcPlane::Xy),
        )?;
        arc.sweep *= fraction;
        arc.end_w = arc.start_w + (arc.end_w - arc.start_w) * fraction;
        arc.length *= fraction;
        stock.sweep_arc(
            tool,
            &arc,
            SweepMode::RemoveMaterial,
            samples,
            None,
            cancellation,
        )?;
    } else {
        stock.sweep_tool(
            tool,
            from,
            lerp(from, to, fraction),
            ToolSweepOptions {
                mode: SweepMode::RemoveMaterial,
                total_samples: samples,
                verification: None,
                cancellation,
            },
        )?;
    }
    Ok(())
}
