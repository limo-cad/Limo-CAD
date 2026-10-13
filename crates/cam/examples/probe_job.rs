//! Read a saved model's JSON (or a CAM document) on stdin. This read-only
//! diagnostic separates cold planning, cached planning and optional stock
//! verification timings. It never changes the source project or posts NC.
use limo_cad_cam::{
    plan_setup, simulate_setup, CamDocumentDto, CamOperationDto, CamSimulationRequestDto,
    CamSimulationTargetDto,
};
use std::{io::Read, time::Instant};

#[path = "support/roughing_audit.rs"]
mod roughing_audit;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = String::new();
    std::io::stdin().read_to_string(&mut source)?;
    let value: serde_json::Value = serde_json::from_str(&source)?;
    let document: CamDocumentDto =
        serde_json::from_value(value.get("cam").unwrap_or(&value).clone())?;
    let setup = document.setups.first().ok_or("No setup")?;
    let started = Instant::now();
    let program = plan_setup(&document, setup.id)?;
    let cold_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let cached = plan_setup(&document, setup.id)?;
    let cached_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(program, cached);
    println!(
        "{}",
        serde_json::json!({"cold_plan_ms":cold_ms,"cached_plan_ms":cached_ms,
        "commands":program.commands.len(),"stats":program.stats,
        "per_operation":program.per_operation,"warnings":program.warnings})
    );
    if std::env::args().any(|arg| arg == "--audit") {
        let started = Instant::now();
        let audit = roughing_audit::audit(&document, &program)?;
        println!(
            "{}",
            serde_json::json!({"target_audit_ms":started.elapsed().as_secs_f64()*1000.0,
            "audit":audit})
        );
    }
    if std::env::args().any(|arg| arg == "--simulate") {
        let targets = setup
            .operations
            .iter()
            .find_map(|op| match op {
                CamOperationDto::Adaptive3d {
                    geometry: Some(g), ..
                } => Some(g.targets.clone()),
                _ => None,
            })
            .ok_or("No captured target meshes")?;
        let args = std::env::args().collect::<Vec<_>>();
        let voxel_size = args
            .windows(2)
            .find(|args| args[0] == "--voxel")
            .map(|args| args[1].parse::<f64>())
            .transpose()?;
        let request = CamSimulationRequestDto {
            setup_id: setup.id,
            voxel_size,
            max_voxels: None,
            stock_mesh: None,
            target: Some(CamSimulationTargetDto {
                cache_key: None,
                meshes: targets,
                tolerance_mm: 0.1,
            }),
            through_operation_id: None,
            completed_steps: None,
            playback_time_seconds: None,
        };
        let started = Instant::now();
        let result = match simulate_setup(&document, &request) {
            Ok(result) => result,
            Err(error) => {
                println!(
                    "{}",
                    serde_json::json!({"simulation_ms":started.elapsed().as_secs_f64()*1000.0,
                    "requested_voxel_mm":voxel_size,"simulation_error":error.to_string()})
                );
                return Err(error.into());
            }
        };
        println!(
            "{}",
            serde_json::json!({"simulation_ms":started.elapsed().as_secs_f64()*1000.0,
            "cells":result.dimensions,"steps":result.steps.len(),"remaining_mm3":result.remaining_volume_mm3,
            "gouge_mm3":result.comparison.as_ref().map(|c| c.gouged_volume_mm3),
            "excess_mm3":result.comparison.as_ref().map(|c| c.excess_volume_mm3),
            "collisions":result.collisions,"warnings":result.warnings})
        );
    }
    Ok(())
}
