//! Exact occurrence interference shared by the native UI and MCP.
use crate::{OcctKernel, PlacedBodyQueryDto};
use nbcad_assembly::{
    broad_phase_interference_pairs, InstanceBodyPoseDto, InterferenceCheckRequestDto,
    InterferencePairResultDto, InterferenceReportDto,
};
use nbcad_solid::SolidSceneDto;

pub fn exact_interference_report(
    kernel: &OcctKernel,
    scene: &SolidSceneDto,
    poses: &[InstanceBodyPoseDto],
    request: &InterferenceCheckRequestDto,
) -> Result<InterferenceReportDto, String> {
    if !scene.errors.is_empty() {
        return Err("Cannot inspect interference while the model has recompute errors".into());
    }
    for id in &request.occurrence_ids {
        if !poses
            .iter()
            .any(|pose| pose.occurrence_id == *id && pose.visible)
        {
            return Err(format!(
                "Occurrence {} is missing, hidden, or has no solid bodies",
                id.0
            ));
        }
    }
    if !request.clearance_threshold_mm.is_finite() || request.clearance_threshold_mm < 0.0 {
        return Err("interference clearance must be finite and non-negative".to_string());
    }
    let mut pairs = Vec::new();
    for (a_index, b_index) in broad_phase_interference_pairs(scene, poses, request)? {
        pairs.push(exact_pair_result(
            kernel,
            &poses[a_index],
            &poses[b_index],
            request.clearance_threshold_mm,
        )?);
    }
    Ok(InterferenceReportDto { exact: true, pairs })
}

pub fn exact_pair_result(
    kernel: &OcctKernel,
    a: &InstanceBodyPoseDto,
    b: &InstanceBodyPoseDto,
    clearance_threshold_mm: f64,
) -> Result<InterferencePairResultDto, String> {
    let exact = kernel
        .exact_interference(
            PlacedBodyQueryDto {
                body_id: a.body_id,
                translation: a.translation,
                rotation: a.rotation,
            },
            PlacedBodyQueryDto {
                body_id: b.body_id,
                translation: b.translation,
                rotation: b.rotation,
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(InterferencePairResultDto {
        occurrence_a: a.occurrence_id,
        body_a: a.body_id,
        occurrence_b: b.occurrence_id,
        body_b: b.body_id,
        minimum_clearance_mm: exact.minimum_clearance_mm,
        overlap_volume_mm3: exact.overlap_volume_mm3,
        closest_point_a: exact.closest_point_a,
        closest_point_b: exact.closest_point_b,
        interfering: exact.overlap_volume_mm3 > 1.0e-7,
        below_clearance: exact.minimum_clearance_mm <= clearance_threshold_mm + 1.0e-7,
    })
}
