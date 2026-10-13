//! Cancellable worker entry point for the same NC interpreter used by the web
//! host. No planner, generated-toolpath cache, or alternate NC model is involved.
use super::*;
use crate::simulation::{simulate_program_with_cancellation, CamSimulationCancellation};

pub fn simulate_gcode_with_cancellation(
    document: &CamDocumentDto,
    request: &CamGcodeSimulationRequestDto,
    cancellation: Option<&CamSimulationCancellation>,
) -> Result<CamSimulationResultDto, CamPlanError> {
    if let Some(cancellation) = cancellation {
        cancellation.check()?;
    }
    document.validate().map_err(CamPlanError)?;
    let setup = document.setup(request.setup_id).ok_or_else(|| {
        CamPlanError(format!(
            "CAM setup {} does not exist for G-code simulation",
            request.setup_id
        ))
    })?;
    let parsed = parse_gcode_with_cancellation(document, setup, request, cancellation)?;
    let simulation_request = CamSimulationRequestDto {
        setup_id: request.setup_id,
        voxel_size: request.voxel_size,
        max_voxels: request.max_voxels,
        stock_mesh: request.stock_mesh.clone(),
        target: request.target.clone(),
        through_operation_id: None,
        completed_steps: request.completed_steps,
        playback_time_seconds: None,
    };
    let result = simulate_program_with_cancellation(
        document,
        setup,
        &parsed.program,
        &simulation_request,
        CamSimulationSourceDto::GCode,
        &parsed.source_lines,
        cancellation,
    )?;
    if let Some(cancellation) = cancellation {
        cancellation.check()?;
    }
    Ok(result)
}
