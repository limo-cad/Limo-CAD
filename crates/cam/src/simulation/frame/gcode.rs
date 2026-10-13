//! NC sessions consume the interpreter's physical timeline, including control
//! compensation, expanded cycles, and original source-line attribution.
use super::*;
use crate::gcode::{simulate_gcode_with_cancellation, CamGcodeSimulationRequestDto};

impl CamPlayback {
    /// Prepare transient NC playback without requiring generated CAM operations.
    /// The parser and complete verification run once on the worker. Subsequent
    /// samples use the same bounded stock sweeps as generated CAM playback.
    pub fn from_gcode(
        document: CamDocumentDto,
        request: CamGcodeSimulationRequestDto,
        start: f64,
        cancellation: Option<&CamSimulationCancellation>,
    ) -> Result<Self, CamPlanError> {
        Self::prepare_gcode(document, request, start, cancellation).map(|(playback, _)| playback)
    }

    /// Prepare playback and complete verification in one worker job. Hosts can
    /// retain the result for reports and transfer the owned session to their
    /// frame worker without running the NC interpreter or full simulation twice.
    pub fn prepare_gcode(
        document: CamDocumentDto,
        mut request: CamGcodeSimulationRequestDto,
        start: f64,
        cancellation: Option<&CamSimulationCancellation>,
    ) -> Result<(Self, CamSimulationResultDto), CamPlanError> {
        if !start.is_finite() || start < 0.0 {
            return Err(CamPlanError("Invalid playback start time".into()));
        }
        request.completed_steps = None;
        let mut complete = simulate_gcode_with_cancellation(&document, &request, cancellation)?;
        let end = complete.estimated_seconds;
        let start = start.min(end);
        let timeline = std::mem::take(&mut complete.steps);
        let stock_mesh = complete.stock_mesh.take();
        let comparison = complete.comparison.take();
        let collisions = std::mem::take(&mut complete.collisions);
        let mut metadata = complete.clone();
        complete.steps = timeline.clone();
        complete.stock_mesh = stock_mesh;
        complete.comparison = comparison;
        complete.collisions = collisions;
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
        let base = initial_stock(
            &document,
            setup,
            &spec,
            request.stock_mesh.as_ref(),
            cancellation,
        )?;
        if let Some(cancellation) = cancellation {
            cancellation.check()?;
        }
        let playback = Self {
            document,
            timeline,
            metadata,
            stock: base.clone(),
            base,
            base_completed: 0,
            completed: 0,
            last_display: None,
            last_display_warnings: Vec::new(),
            tiles: None,
            start,
            end,
        };
        Ok((playback, complete))
    }
}

#[cfg(test)]
mod tests;
