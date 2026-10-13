//! Presentation adapter for the existing shared NC simulator. No planner fallback.
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn prepare(
    document: &CamDocumentDto,
    input: &nc_input::Input,
    request: CamSimulationRequestDto,
    cancellation: &CamSimulationCancellation,
    warning: Option<String>,
) -> Result<Prepared, String> {
    let request = input.request(request);
    let file_name = request
        .file_name
        .clone()
        .unwrap_or_else(|| "program.nc".into());
    let (kernel, mut simulation) =
        limo_cad_cam::CamPlayback::prepare_gcode(document.clone(), request, 0., Some(cancellation))
            .map_err(|error| error.to_string())?;
    if cancellation.is_cancelled() {
        return Err("NC simulation cancelled".into());
    }
    let stock = crate::retained_cam_stock(&simulation);
    simulation.stock_mesh = None;
    simulation.native_stock_present = stock.is_some();
    static NEXT_PATH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 63);
    let path_id = NEXT_PATH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let paths = timeline::paths(&simulation, path_id, 0)?;
    let tool = timeline::pose(document, &simulation, path_id, simulation.estimated_seconds)?.0;
    let message = format!(
        "Simulation: {:.1} s · {} contacts · {:.1} mm³ removed · NC {} · voxel {:.3} mm",
        simulation.estimated_seconds,
        simulation.collisions.len(),
        simulation.removed_volume_mm3,
        file_name,
        simulation.cell_size[0]
    );
    let mut details = format!("{message}\nController language: {:?}\nNC blocks use the program's N sequence number when present, otherwise its physical source line.\n", input.dialect);
    let mut seen = std::collections::HashSet::new();
    for warning in warning.iter().chain(simulation.warnings.iter()) {
        if seen.insert(warning) {
            details.push_str(&format!("\nWarning: {warning}\n"));
        }
    }
    if let Some(comparison) = &simulation.comparison {
        details.push_str(&format!("\nTarget comparison\nRequested tolerance: {:.3} mm\nEffective voxel tolerance: {:.3} mm\nExcess material: {:.3} mm³\nGouged target: {:.3} mm³\nInitial stock shortfall: {:.3} mm³\n",
            comparison.requested_tolerance_mm, comparison.effective_tolerance_mm,
            comparison.excess_volume_mm3, comparison.gouged_volume_mm3, comparison.initial_shortfall_volume_mm3));
    }
    let source_lines: std::collections::HashMap<_, _> = simulation
        .steps
        .iter()
        .filter_map(|step| step.source_line.map(|line| (step.command_index, line)))
        .collect();
    for (index, collision) in simulation.collisions.iter().enumerate() {
        let source = source_lines.get(&collision.command_index).map_or_else(
            || "unmapped NC block".into(),
            |line| format!("NC block {line}"),
        );
        details.push_str(&format!(
            "\nContact {} · {source} · setup X {:.3}, Y {:.3}, Z {:.3} mm\n{}\n",
            index + 1,
            collision.position.x,
            collision.position.y,
            collision.position.z,
            collision.message
        ));
    }
    Ok(Prepared {
        paths,
        tool,
        simulation: Some(simulation),
        stock,
        message,
        details,
        path_id,
        start_time: 0.,
        nc_kernel: Some(Mutex::new(kernel)),
    })
}
