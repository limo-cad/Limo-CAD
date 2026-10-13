//! Limo CAD native desktop: one Bevy host over the shared engine and MCP services.

mod app_config;
mod app_preferences;
mod cam_library;
mod cam_posts;
mod native_editor;
mod native_forms;
mod native_print;
pub mod native_viewport;
mod recipe_links;
mod session_bridge;
mod six_dof_mouse;
mod state;

use limo_cad_cam::CamSimulationResultDto;
use native_viewport::ViewportCamStock;

fn retained_cam_stock(result: &CamSimulationResultDto) -> Option<ViewportCamStock> {
    let mesh = result.stock_mesh.as_ref()?;
    if mesh.positions.is_empty() || !mesh.positions.len().is_multiple_of(3) {
        return None;
    }
    let wcs = result.wcs;
    let mut positions = Vec::with_capacity(mesh.positions.len());
    for point in mesh.positions.as_chunks::<3>().0 {
        let x = point[0] as f64;
        let y = point[1] as f64;
        let z = point[2] as f64;
        positions.extend([
            (wcs.origin.x + x * wcs.x_axis[0] + y * wcs.y_axis[0] + z * wcs.z_axis[0]) as f32,
            (wcs.origin.y + x * wcs.x_axis[1] + y * wcs.y_axis[1] + z * wcs.z_axis[1]) as f32,
            (wcs.origin.z + x * wcs.x_axis[2] + y * wcs.y_axis[2] + z * wcs.z_axis[2]) as f32,
        ]);
    }
    let mut normals = Vec::new();
    if mesh.normals.len() == mesh.positions.len() {
        normals.reserve(mesh.normals.len());
        for normal in mesh.normals.as_chunks::<3>().0 {
            let x = normal[0] as f64;
            let y = normal[1] as f64;
            let z = normal[2] as f64;
            let model = [
                x * wcs.x_axis[0] + y * wcs.y_axis[0] + z * wcs.z_axis[0],
                x * wcs.x_axis[1] + y * wcs.y_axis[1] + z * wcs.z_axis[1],
                x * wcs.x_axis[2] + y * wcs.y_axis[2] + z * wcs.z_axis[2],
            ];
            let length = model.iter().map(|value| value * value).sum::<f64>().sqrt();
            if length <= f64::EPSILON {
                normals.extend([0.0, 0.0, 1.0]);
            } else {
                normals.extend(model.map(|value| (value / length) as f32));
            }
        }
    }
    Some(ViewportCamStock {
        positions: std::sync::Arc::new(positions),
        normals: std::sync::Arc::new(normals),
        time_seconds: None,
    })
}
