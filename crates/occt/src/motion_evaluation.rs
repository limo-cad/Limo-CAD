//! Shared contact-aware motion sampling for native UI and headless MCP.
use crate::{exact_pair_result, OcctKernel};
use limo_cad_sketch::{
    approximate_pair_result, contact_violation_score, ContactSetDto, EvaluateMotionStudyRequestDto,
    InterferencePairResultDto, InterferenceReportDto, MotionStudyEvaluationDto, MotionStudyId,
    MotionStudySampleDto, SampleMotionStudyRequestDto, SketchManager,
};
use limo_cad_solid::SolidSceneDto;

pub fn evaluate_motion_study(
    manager: &SketchManager,
    kernel: &OcctKernel,
    request: &EvaluateMotionStudyRequestDto,
) -> Result<MotionStudyEvaluationDto, String> {
    // Sampling is read-only for this call's entire lifetime. In particular,
    // playback must not duplicate every mesh buffer at each sampled frame.
    let document = manager.assembly_document_ref();
    let scene = manager.solid_scene_ref();
    let sample = |time_seconds| {
        manager
            .sample_motion_study(SampleMotionStudyRequestDto {
                study_id: request.study_id,
                time_seconds,
            })
            .map_err(|e| e.to_string())
            .and_then(|s| {
                if s.solution.solved {
                    Ok(s)
                } else {
                    Err("Cannot preview an unsolved motion sample".into())
                }
            })
    };
    let mut final_sample = sample(request.time_seconds)?;
    let mut stopped_by_contact = None;
    let contacts = document
        .contact_sets
        .iter()
        .filter(|c| c.enabled)
        .collect::<Vec<_>>();
    if request.enforce_contacts && contacts.iter().any(|c| c.stop_motion) {
        let start = sample(request.previous_time_seconds.unwrap_or(0.))?;
        for contact in contacts.iter().copied().filter(|c| c.stop_motion) {
            let start_violation = gated_exact_contact_violation(kernel, scene, &start, contact)?;
            let end_violation =
                gated_exact_contact_violation(kernel, scene, &final_sample, contact)?;
            if start_violation > 1e-7 && end_violation >= start_violation {
                final_sample = start.clone();
                stopped_by_contact = Some(contact.id);
                break;
            }
            if start_violation <= 1e-7 {
                if let Some(crossing) = first_exact_contact_crossing(
                    manager,
                    kernel,
                    scene,
                    request.study_id,
                    (&start, &final_sample),
                    contact,
                    end_violation,
                )? {
                    final_sample = crossing;
                    stopped_by_contact = Some(contact.id);
                }
            }
        }
    }
    let mut pairs = Vec::with_capacity(contacts.len());
    let mut exact = true;
    for contact in contacts {
        let (pair, is_exact) = gated_contact_result(kernel, scene, &final_sample, contact)?;
        pairs.push(pair);
        exact &= is_exact;
    }
    Ok(MotionStudyEvaluationDto {
        stop_time_seconds: stopped_by_contact.map(|_| final_sample.time_seconds),
        sample: final_sample,
        contacts: InterferenceReportDto { exact, pairs },
        stopped_by_contact,
    })
}

fn gated_exact_contact_violation(
    kernel: &OcctKernel,
    scene: &SolidSceneDto,
    sample: &MotionStudySampleDto,
    contact: &ContactSetDto,
) -> Result<f64, String> {
    let (result, _) = gated_contact_result(kernel, scene, sample, contact)?;
    Ok(contact_violation_score(&result, contact.clearance_mm))
}

fn gated_contact_result(
    kernel: &OcctKernel,
    scene: &SolidSceneDto,
    sample: &MotionStudySampleDto,
    contact: &ContactSetDto,
) -> Result<(InterferencePairResultDto, bool), String> {
    if !sample.solution.solved {
        return Err("Cannot evaluate contacts in an unsolved motion sample".into());
    }
    let a = sample
        .solution
        .instance_body_poses
        .iter()
        .find(|pose| pose.occurrence_id == contact.occurrence_a && pose.body_id == contact.body_a)
        .ok_or_else(|| format!("contact '{}' first placed body is missing", contact.name))?;
    let b = sample
        .solution
        .instance_body_poses
        .iter()
        .find(|pose| pose.occurrence_id == contact.occurrence_b && pose.body_id == contact.body_b)
        .ok_or_else(|| format!("contact '{}' second placed body is missing", contact.name))?;
    let broad = approximate_pair_result(scene, a, b, contact.clearance_mm)?;
    if contact_violation_score(&broad, contact.clearance_mm) <= 1.0e-7 {
        return Ok((broad, false));
    }
    let exact = exact_pair_result(kernel, a, b, contact.clearance_mm)?;
    Ok((exact, true))
}

/// Search the full frame interval rather than only its endpoints. Cheap mesh
/// bounds gate all OCCT work; exact B-rep checks happen only while the chosen
/// contact pair can actually touch. Eight ordered probes catch short
/// enter/exit events across a normal 30 Hz playback frame before bisection.
fn first_exact_contact_crossing(
    manager: &SketchManager,
    kernel: &OcctKernel,
    scene: &SolidSceneDto,
    study_id: MotionStudyId,
    (start, end): (&MotionStudySampleDto, &MotionStudySampleDto),
    contact: &ContactSetDto,
    end_violation: f64,
) -> Result<Option<MotionStudySampleDto>, String> {
    const PROBE_STEPS: usize = 8;
    const BISECTION_STEPS: usize = 18;
    let mut safe_time = start.time_seconds;
    for step in 1..=PROBE_STEPS {
        let fraction = step as f64 / PROBE_STEPS as f64;
        let time = start.time_seconds + (end.time_seconds - start.time_seconds) * fraction;
        let sample = if step == PROBE_STEPS {
            end.clone()
        } else {
            manager
                .sample_motion_study(SampleMotionStudyRequestDto {
                    study_id,
                    time_seconds: time,
                })
                .map_err(|error| error.to_string())?
        };
        let violation = if step == PROBE_STEPS {
            end_violation
        } else {
            gated_exact_contact_violation(kernel, scene, &sample, contact)?
        };
        if violation <= 1.0e-7 {
            safe_time = sample.time_seconds;
            continue;
        }

        let mut safe = safe_time;
        let mut blocked = sample.time_seconds;
        for _ in 0..BISECTION_STEPS {
            let middle = (safe + blocked) * 0.5;
            let candidate = manager
                .sample_motion_study(SampleMotionStudyRequestDto {
                    study_id,
                    time_seconds: middle,
                })
                .map_err(|error| error.to_string())?;
            if gated_exact_contact_violation(kernel, scene, &candidate, contact)? > 1.0e-7 {
                blocked = middle;
            } else {
                safe = middle;
            }
        }
        return manager
            .sample_motion_study(SampleMotionStudyRequestDto {
                study_id,
                time_seconds: blocked,
            })
            .map(Some)
            .map_err(|error| error.to_string());
    }
    Ok(None)
}
