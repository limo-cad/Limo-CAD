//! One exact swept-interference implementation for desktop and headless MCP.
use crate::{exact_interference_report, OcctKernel};
use limo_cad_sketch::{
    InterferenceCheckRequestDto, SampleMotionStudyRequestDto, SketchManager,
    SweptCollisionEventDto, SweptCollisionReportDto, SweptCollisionRequestDto,
};
use std::collections::BTreeMap;

pub fn exact_swept_collision_check(
    manager: &SketchManager,
    kernel: &OcctKernel,
    request: &SweptCollisionRequestDto,
) -> Result<SweptCollisionReportDto, String> {
    if !request.sample_rate_hz.is_finite() || !(1. ..=240.).contains(&request.sample_rate_hz) {
        return Err("swept collision sample rate must be between 1 and 240 Hz".into());
    }
    if !request.clearance_threshold_mm.is_finite() || request.clearance_threshold_mm < 0. {
        return Err("swept collision clearance must be finite and non-negative".into());
    }
    let document = manager.assembly_document_ref();
    let study = document
        .motion_studies
        .iter()
        .find(|s| s.id == request.study_id)
        .ok_or_else(|| format!("motion study {} does not exist", request.study_id.0))?;
    let steps = (study.duration_seconds * request.sample_rate_hz).ceil();
    if !steps.is_finite() || !(0. ..=100_000.).contains(&steps) {
        return Err("swept collision study exceeds 100,001 samples".into());
    }
    let count = steps as u32 + 1;
    // The manager is immutably borrowed throughout inspection; retaining an
    // owned scene here would duplicate all mesh buffers without isolation gain.
    let scene = manager.solid_scene_ref();
    let mut events = BTreeMap::<(u64, u64, u64, u64), SweptCollisionEventDto>::new();
    let mut sample_count = 0;
    for index in 0..count {
        let time = (index as f64 / request.sample_rate_hz).min(study.duration_seconds);
        let sample = manager
            .sample_motion_study(SampleMotionStudyRequestDto {
                study_id: request.study_id,
                time_seconds: time,
            })
            .map_err(|e| e.to_string())?;
        if !sample.solution.solved {
            return Err("Cannot inspect interference in an unsolved motion sample".into());
        }
        let report = exact_interference_report(
            kernel,
            scene,
            &sample.solution.instance_body_poses,
            &InterferenceCheckRequestDto {
                occurrence_ids: vec![],
                clearance_threshold_mm: request.clearance_threshold_mm,
            },
        )?;
        sample_count += 1;
        for pair in report
            .pairs
            .into_iter()
            .filter(|p| p.interfering || p.below_clearance)
        {
            let key = (
                pair.occurrence_a.0,
                pair.body_a.0,
                pair.occurrence_b.0,
                pair.body_b.0,
            );
            events
                .entry(key)
                .and_modify(|e| {
                    e.last_time_seconds = time;
                    e.minimum_clearance_mm = e.minimum_clearance_mm.min(pair.minimum_clearance_mm);
                    e.maximum_overlap_volume_mm3 =
                        e.maximum_overlap_volume_mm3.max(pair.overlap_volume_mm3);
                })
                .or_insert(SweptCollisionEventDto {
                    occurrence_a: pair.occurrence_a,
                    body_a: pair.body_a,
                    occurrence_b: pair.occurrence_b,
                    body_b: pair.body_b,
                    first_time_seconds: time,
                    last_time_seconds: time,
                    minimum_clearance_mm: pair.minimum_clearance_mm,
                    maximum_overlap_volume_mm3: pair.overlap_volume_mm3,
                });
        }
        if request.stop_at_first && !events.is_empty() {
            break;
        }
    }
    let mut events = events.into_values().collect::<Vec<_>>();
    events.sort_by(|a, b| a.first_time_seconds.total_cmp(&b.first_time_seconds));
    Ok(SweptCollisionReportDto {
        exact: true,
        sample_count,
        events,
    })
}
