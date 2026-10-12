//! noBS CAD native shell (Tauri 2).
//!
//! Thin IPC layer over the engine crates (`nbcad-core` /
//! `nbcad-sketch`): the shell owns the [`state::AppState`] and exposes
//! the engine API as JSON-string commands. Every `engine_*` command
//! dispatches through `nbcad_sketch::host::handle` — the exact code
//! path the WASM host uses, so native and browser behavior are identical.
//! All modeling logic lives in the engine crates, never here.

mod cam_library;
mod cam_playback;
mod cam_posts;
pub mod launch_requests;
mod native_menu;
pub mod native_viewport;
mod recipe_links;
mod scripts;
mod session_bridge;
mod six_dof_mouse;
pub mod startup;
pub mod startup_metrics;
mod state;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

use native_viewport::{
    NativePick, NativePickPurpose, NativeViewport, NativeViewportMetrics, ViewportCamStock,
    ViewportCamera, ViewportLayout, ViewportModel, ViewportPresentation, ViewportPreview,
};
use nbcad_cam::{
    simulate_setup_with_cancellation, CamSimulationCancellation, CamSimulationRequestDto,
    CamSimulationResultDto,
};
use nbcad_core::DocumentDto;
use nbcad_sketch::{err_json, ok_json};
use serde::Serialize;
use six_dof_mouse::SixDofMouseState;
use state::{AppState, BOOTSTRAP_SESSION_ID};
use tauri::{Emitter, Manager};

/// Health-check command used by the frontend IPC wrapper.
#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

#[tauri::command]
fn native_build_info() -> nbcad_core::BuildInfo {
    nbcad_core::build_info()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemMemoryStatus {
    total_bytes: u64,
    available_bytes: u64,
    pressure: &'static str,
}

/// Native authority for the application-wide unsaved-work guard. Approved
/// frontend exits also drain the embedded stdio response before termination.
#[derive(Default)]
struct NativeQuitState {
    unsaved: AtomicBool,
    approved: AtomicBool,
}

#[derive(Default)]
struct CamSimulationService {
    generation: AtomicU64,
    current: Mutex<Option<(u64, CamSimulationCancellation)>>,
}

impl CamSimulationService {
    fn begin(&self) -> (u64, CamSimulationCancellation) {
        let request_id = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let cancellation = CamSimulationCancellation::default();
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, previous)) = current.replace((request_id, cancellation.clone())) {
            previous.cancel();
        }
        (request_id, cancellation)
    }

    fn with_current<T>(&self, request_id: u64, action: impl FnOnce() -> T) -> Option<T> {
        let current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        current
            .as_ref()
            .is_some_and(|(current_id, _)| *current_id == request_id)
            .then(action)
    }
}

#[tauri::command]
fn native_unsaved_set(state: tauri::State<'_, NativeQuitState>, unsaved: bool) {
    state.unsaved.store(unsaved, Ordering::Release);
}

#[tauri::command]
async fn native_force_quit(app: tauri::AppHandle) {
    // The frontend's control receipt must reach the caller's actual stdio pipe,
    // not just its inbox file. Keep the event loop free while that response
    // finishes; a host that stops reading must not prevent the user from exiting.
    let _ = tauri::async_runtime::spawn_blocking(|| {
        nbcad_mcp::shutdown_desktop_stdio(std::time::Duration::from_secs(3))
    })
    .await;
    // Do not log to a potentially blocked pipe on the timeout path: exiting
    // must remain bounded even when the host has stopped reading both streams.
    app.state::<NativeQuitState>()
        .approved
        .store(true, Ordering::Release);
    app.exit(0);
}

/// Portable physical-memory pressure estimate used by the tab retention
/// policy. sysinfo has native macOS and Windows backends; conservative
/// thresholds avoid evicting professional documents during normal caching.
#[tauri::command]
fn system_memory_status() -> SystemMemoryStatus {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let total_bytes = system.total_memory();
    let available_bytes = system.available_memory();
    let critical_threshold = (total_bytes / 20).max(512 * 1024 * 1024);
    let constrained_threshold = (total_bytes / 10).max(1024 * 1024 * 1024);
    let pressure = if available_bytes <= critical_threshold {
        "critical"
    } else if available_bytes <= constrained_threshold {
        "constrained"
    } else {
        "normal"
    };
    SystemMemoryStatus {
        total_bytes,
        available_bytes,
        pressure,
    }
}

/// Snapshot of the current document (name, settings, browser tree).
#[tauri::command]
fn get_document(state: tauri::State<'_, AppState>) -> DocumentDto {
    state.document_snapshot()
}

#[tauri::command]
fn native_viewport_set_layout(
    app: tauri::AppHandle,
    viewport: tauri::State<'_, NativeViewport>,
    layout: ViewportLayout,
) -> Result<(), String> {
    viewport.set_layout(&app, layout)
}

#[tauri::command]
fn native_viewport_set_suspended(
    app: tauri::AppHandle,
    viewport: tauri::State<'_, NativeViewport>,
    suspended: bool,
) -> Result<(), String> {
    viewport.set_suspended(&app, suspended)
}

#[tauri::command]
fn native_viewport_start(viewport: tauri::State<'_, NativeViewport>) -> Result<(), String> {
    viewport.start()
}

/// Expand the existing startup window only after its real viewport is prepared.
/// Keep all window ownership on the UI thread; the caller awaits the request,
/// then independently verifies a presented frame at the resulting DOM layout.
#[tauri::command]
async fn native_window_prepare_workspace(app: tauri::AppHandle) -> Result<(), String> {
    startup_metrics::record_native_milestone("workspaceExpansionRequested");
    let (send, receive) = tokio::sync::oneshot::channel();
    let window_app = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let window = window_app
                .get_webview_window("main")
                .ok_or("main window is unavailable")?;
            // Restore normal workspace chrome before applying the client size.
            // Native decorations can change the client extent independently of
            // the outer window, so frontend readiness still checks its new frame.
            window
                .set_decorations(true)
                .map_err(|error| error.to_string())?;
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            window.set_shadow(true).map_err(|error| error.to_string())?;
            // Enable resizing before changing constraints on GTK/Linux.
            window
                .set_resizable(true)
                .map_err(|error| error.to_string())?;
            window
                .set_size(tauri::LogicalSize::new(1440.0, 900.0))
                .map_err(|error| error.to_string())?;
            window
                .set_min_size(Some(tauri::LogicalSize::new(1200.0, 760.0)))
                .map_err(|error| error.to_string())?;
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            window
                .set_maximizable(true)
                .map_err(|error| error.to_string())?;
            window.center().map_err(|error| error.to_string())?;
            Ok::<_, String>(())
        })();
        let _ = send.send(result);
    })
    .map_err(|error| error.to_string())?;
    receive
        .await
        .map_err(|_| "workspace window request was interrupted".to_string())?
}

#[tauri::command]
fn native_window_set_locale(app: tauri::AppHandle, locale: String) -> Result<(), String> {
    let title = match locale.as_str() {
        "zh-CN" => "砺模 CAD",
        "en" | "es" | "de" => "Limo CAD",
        _ => return Err("unsupported application locale".to_string()),
    };
    app.state::<native_menu::NativeFileMenuState>()
        .set_new_window_locale(&locale)?;
    app.get_webview_window("main")
        .ok_or("main window is unavailable")?
        .set_title(title)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn native_viewport_sync_model(
    engine: tauri::State<'_, AppState>,
    viewport: tauri::State<'_, NativeViewport>,
) -> Result<u64, String> {
    let (
        session_id,
        geometry_revision,
        scene,
        active_sketch,
        finished_sketches,
        datum_planes,
        profile_catalog,
        body_appearances,
        body_poses,
        instance_body_poses,
    ) = engine.viewport_snapshot();
    viewport.sync_model(ViewportModel {
        session_id,
        geometry_revision,
        scene,
        active_sketch,
        finished_sketches,
        datum_planes,
        profile_catalog,
        body_appearances,
        body_poses,
        instance_body_poses,
    })?;
    Ok(geometry_revision)
}

#[tauri::command]
async fn native_viewport_set_camera(
    viewport: tauri::State<'_, NativeViewport>,
    camera: ViewportCamera,
) -> Result<(), String> {
    viewport.set_camera(camera)
}

#[tauri::command]
async fn native_viewport_set_preview(
    viewport: tauri::State<'_, NativeViewport>,
    preview: ViewportPreview,
) -> Result<(), String> {
    viewport.set_preview(preview)
}

#[tauri::command]
async fn native_viewport_set_presentation(
    viewport: tauri::State<'_, NativeViewport>,
    presentation: ViewportPresentation,
) -> Result<(), String> {
    viewport.set_presentation(presentation)
}

#[tauri::command]
async fn native_viewport_pick(
    viewport: tauri::State<'_, NativeViewport>,
    x: f32,
    y: f32,
    camera: Option<ViewportCamera>,
    logical_width: Option<f32>,
    logical_height: Option<f32>,
    purpose: Option<NativePickPurpose>,
) -> Result<Option<NativePick>, String> {
    let logical_size = logical_width.zip(logical_height);
    viewport.pick(x, y, camera, logical_size, purpose.unwrap_or_default())
}

#[tauri::command]
fn native_viewport_metrics(viewport: tauri::State<'_, NativeViewport>) -> NativeViewportMetrics {
    viewport.metrics()
}

fn retained_cam_stock(result: &CamSimulationResultDto) -> Option<ViewportCamStock> {
    let mesh = result.stock_mesh.as_ref()?;
    if mesh.positions.is_empty() || !mesh.positions.len().is_multiple_of(3) {
        return None;
    }
    let wcs = result.wcs;
    let mut positions = Vec::with_capacity(mesh.positions.len());
    for point in mesh.positions.chunks_exact(3) {
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
        for normal in mesh.normals.chunks_exact(3) {
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
    })
}

/// Desktop fast path: clone CAM intent under the engine lock, then perform all
/// voxel/cutter work on a blocking Rust worker. Only metadata returns through
/// JSON; the large remaining-stock mesh goes straight into retained Bevy GPU
/// geometry. A newer call cooperatively cancels this one.
#[tauri::command]
async fn engine_cam_simulate(
    state: tauri::State<'_, AppState>,
    viewport: tauri::State<'_, NativeViewport>,
    service: tauri::State<'_, CamSimulationService>,
    playback: tauri::State<'_, cam_playback::CamPlaybackService>,
    payload: String,
) -> Result<String, String> {
    let request = match serde_json::from_str::<CamSimulationRequestDto>(&payload) {
        Ok(request) => request,
        Err(error) => return Ok(err_json(format!("invalid CAM simulation request: {error}"))),
    };
    let (session_id, document, safety_warning) = match state.cam_snapshot(request.setup_id) {
        Ok(snapshot) => snapshot,
        Err(error) => return Ok(err_json(error)),
    };
    let (request_id, cancellation) = service.begin();
    playback.clear();
    // Keep the retained surface until its replacement is ready. Presentation
    // freshness gates hide it while changing scope; hover must not clear it.
    let worker_cancellation = cancellation.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        simulate_setup_with_cancellation(&document, &request, Some(&worker_cancellation))
    })
    .await;
    let mut result = match worker {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => return Ok(err_json(error.to_string())),
        Err(error) => return Ok(err_json(format!("CAM simulation worker failed: {error}"))),
    };
    if let Some(warning) = safety_warning {
        result.warnings.insert(0, warning);
    }
    if state.active_project_session_id() != session_id {
        return Ok(err_json(
            "CAM simulation project changed before the result was ready",
        ));
    }
    let retained = retained_cam_stock(&result);
    let published = service.with_current(request_id, || {
        retained
            .map(|stock| viewport.set_cam_stock(Some(stock)).is_ok())
            .unwrap_or(false)
    });
    let Some(published) = published else {
        return Ok(err_json("CAM simulation superseded by a newer request"));
    };
    if published {
        result.stock_mesh = None;
        result.native_stock_present = true;
        if let Some(comparison) = &mut result.comparison {
            // Verification remains authoritative in the counts/volumes and
            // issue list. The normal stage view intentionally shows only
            // remaining stock, so these hidden evidence meshes must not pay a
            // second large JSON transfer.
            comparison.excess_mesh = None;
            comparison.gouge_mesh = None;
        }
    }
    Ok(ok_json(result))
}

macro_rules! engine_command {
    ($name:ident, $method:literal) => {
        engine_command!($name, $method, mutate);
    };
    ($name:ident, $method:literal, no_payload) => {
        engine_command!($name, $method, no_payload, mutate);
    };
    ($name:ident, $method:literal, read) => {
        /// Read-only engine command — see `nbcad_sketch::host::handle`.
        #[tauri::command]
        fn $name(state: tauri::State<'_, AppState>, payload: &str) -> String {
            state.engine_call($method, payload)
        }
    };
    ($name:ident, $method:literal, no_payload, read) => {
        /// Read-only engine command — see `nbcad_sketch::host::handle`.
        #[tauri::command]
        fn $name(state: tauri::State<'_, AppState>) -> String {
            state.engine_call($method, "")
        }
    };
    ($name:ident, $method:literal, mutate) => {
        /// Mutating engine command — bumps session `engine_revision` under the
        /// publisher lock on success (same critical section as inbox apply).
        #[tauri::command]
        fn $name(
            window: tauri::WebviewWindow,
            bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
            state: tauri::State<'_, AppState>,
            payload: &str,
        ) -> String {
            bridge.run_ui_mutation(window.label(), || state.engine_call($method, payload))
        }
    };
    ($name:ident, $method:literal, no_payload, mutate) => {
        /// Mutating engine command — bumps session `engine_revision` under the
        /// publisher lock on success (same critical section as inbox apply).
        #[tauri::command]
        fn $name(
            window: tauri::WebviewWindow,
            bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
            state: tauri::State<'_, AppState>,
        ) -> String {
            bridge.run_ui_mutation(window.label(), || state.engine_call($method, ""))
        }
    };
}

engine_command!(engine_begin_sketch, "begin_sketch");
engine_command!(engine_document_set_name, "document_set_name");
#[tauri::command]
fn engine_project_export_model(
    state: tauri::State<'_, AppState>,
    payload: Option<String>,
) -> String {
    state.engine_call("project_export_model", payload.as_deref().unwrap_or(""))
}
engine_command!(engine_end_sketch, "end_sketch", no_payload);
engine_command!(
    engine_finished_sketches,
    "finished_sketches",
    no_payload,
    read
);
engine_command!(engine_edit_sketch, "edit_sketch");
engine_command!(engine_active_sketch, "active_sketch", no_payload, read);
engine_command!(engine_profile_catalog, "profile_catalog", no_payload, read);
engine_command!(engine_solid_scene, "solid_scene", no_payload, read);
engine_command!(
    engine_body_appearances,
    "body_appearances",
    no_payload,
    read
);
engine_command!(
    engine_project_visibility,
    "project_visibility",
    no_payload,
    read
);
engine_command!(engine_project_set_visibility, "project_set_visibility");
engine_command!(engine_named_views, "named_views", no_payload, read);
engine_command!(engine_upsert_named_view, "upsert_named_view");
engine_command!(engine_rename_named_view, "rename_named_view");
engine_command!(engine_delete_named_view, "delete_named_view");
engine_command!(engine_set_named_views, "set_named_views");
engine_command!(engine_recall_named_view, "recall_named_view");
engine_command!(engine_clear_named_view, "clear_named_view", no_payload);
engine_command!(
    engine_construction_set_visibility,
    "construction_set_visibility"
);
engine_command!(
    engine_drawing_document,
    "drawing_document",
    no_payload,
    read
);
engine_command!(engine_drawing_apply, "drawing_apply");
engine_command!(engine_drawing_set_document, "drawing_set_document");
engine_command!(
    engine_assembly_document,
    "assembly_document",
    no_payload,
    read
);
engine_command!(engine_assembly_set_document, "assembly_set_document");
engine_command!(
    engine_assembly_solution,
    "assembly_solution",
    no_payload,
    read
);
engine_command!(
    engine_assembly_create_component,
    "assembly_create_component"
);
engine_command!(
    engine_assembly_update_component,
    "assembly_update_component"
);
engine_command!(
    engine_assembly_create_occurrence,
    "assembly_create_occurrence"
);
engine_command!(
    engine_assembly_update_occurrence,
    "assembly_update_occurrence"
);
engine_command!(
    engine_assembly_duplicate_occurrence,
    "assembly_duplicate_occurrence"
);
engine_command!(
    engine_assembly_set_occurrence_grounded,
    "assembly_set_occurrence_grounded"
);
engine_command!(
    engine_assembly_set_occurrence_pose,
    "assembly_set_occurrence_pose"
);
engine_command!(
    engine_assembly_preview_joint,
    "assembly_preview_joint",
    read
);
engine_command!(engine_assembly_create_joint, "assembly_create_joint");
engine_command!(engine_assembly_update_joint, "assembly_update_joint");
engine_command!(
    engine_assembly_preview_joint_update,
    "assembly_preview_joint_update",
    read
);
engine_command!(engine_assembly_delete_joint, "assembly_delete_joint");
engine_command!(
    engine_assembly_set_joint_enabled,
    "assembly_set_joint_enabled"
);
engine_command!(
    engine_assembly_set_joint_motion,
    "assembly_set_joint_motion"
);
engine_command!(
    engine_assembly_preview_joint_motion,
    "assembly_preview_joint_motion",
    read
);
engine_command!(
    engine_assembly_set_grounded_body,
    "assembly_set_grounded_body"
);
engine_command!(
    engine_assembly_set_joint_coordinates,
    "assembly_set_joint_coordinates"
);
engine_command!(
    engine_assembly_preview_joint_coordinates,
    "assembly_preview_joint_coordinates",
    read
);
engine_command!(
    engine_assembly_preview_mechanism_drag,
    "assembly_preview_mechanism_drag",
    read
);
engine_command!(
    engine_assembly_apply_joint_motions,
    "assembly_apply_joint_motions"
);
engine_command!(engine_assembly_create_position, "assembly_create_position");
engine_command!(engine_assembly_update_position, "assembly_update_position");
engine_command!(engine_assembly_delete_position, "assembly_delete_position");
engine_command!(engine_assembly_apply_position, "assembly_apply_position");
engine_command!(
    engine_assembly_create_motion_study,
    "assembly_create_motion_study"
);
engine_command!(
    engine_assembly_update_motion_study,
    "assembly_update_motion_study"
);
engine_command!(
    engine_assembly_delete_motion_study,
    "assembly_delete_motion_study"
);
engine_command!(
    engine_assembly_sample_motion_study,
    "assembly_sample_motion_study",
    read
);
engine_command!(
    engine_assembly_export_motion_path_csv,
    "assembly_export_motion_path_csv",
    read
);
engine_command!(
    engine_assembly_create_contact_set,
    "assembly_create_contact_set"
);
engine_command!(
    engine_assembly_update_contact_set,
    "assembly_update_contact_set"
);
engine_command!(
    engine_assembly_delete_contact_set,
    "assembly_delete_contact_set"
);

#[tauri::command]
fn engine_assembly_interference_check(state: tauri::State<'_, AppState>, payload: &str) -> String {
    state.assembly_interference_check(payload)
}

#[tauri::command]
fn engine_assembly_evaluate_motion_study(
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    state.assembly_evaluate_motion_study(payload)
}

#[tauri::command]
fn engine_assembly_swept_collision_check(
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    state.assembly_swept_collision_check(payload)
}
engine_command!(engine_geometry_edge_chain, "geometry_edge_chain", read);
engine_command!(engine_cam_chamfer_geometry, "cam_chamfer_geometry", read);
engine_command!(engine_cam_cutter_mesh, "cam_cutter_mesh", read);
engine_command!(engine_cam_document, "cam_document", no_payload);
engine_command!(engine_cam_set_document, "cam_set_document");
engine_command!(
    engine_cam_toolpath_statuses,
    "cam_toolpath_statuses",
    no_payload,
    read
);
engine_command!(engine_cam_regenerate_operation, "cam_regenerate_operation");
engine_command!(engine_cam_regenerate_setup, "cam_regenerate_setup");
engine_command!(engine_cam_plan, "cam_plan");
engine_command!(engine_cam_post, "cam_post");
engine_command!(engine_cam_analyze_nbpost, "cam_analyze_nbpost");
engine_command!(engine_cam_simulate_gcode, "cam_simulate_gcode");
engine_command!(engine_cam_post_events, "cam_post_events");
engine_command!(engine_set_body_appearance, "set_body_appearance");

#[tauri::command]
fn engine_drawing_projection(state: tauri::State<'_, AppState>, payload: &str) -> String {
    state.drawing_projection(payload)
}
#[tauri::command]
fn engine_drawing_export(state: tauri::State<'_, AppState>, payload: &str) -> String {
    state.drawing_export(payload)
}
engine_command!(
    engine_extrude_definitions,
    "extrude_definitions",
    no_payload,
    read
);
engine_command!(
    engine_revolve_definitions,
    "revolve_definitions",
    no_payload,
    read
);
engine_command!(
    engine_sweep_definitions,
    "sweep_definitions",
    no_payload,
    read
);
engine_command!(
    engine_loft_definitions,
    "loft_definitions",
    no_payload,
    read
);
engine_command!(engine_rib_definitions, "rib_definitions", no_payload, read);
engine_command!(
    engine_fillet_definitions,
    "fillet_definitions",
    no_payload,
    read
);
engine_command!(
    engine_chamfer_definitions,
    "chamfer_definitions",
    no_payload,
    read
);
engine_command!(
    engine_hole_definitions,
    "hole_definitions",
    no_payload,
    read
);
engine_command!(
    engine_datum_plane_definitions,
    "datum_plane_definitions",
    no_payload,
    read
);
engine_command!(
    engine_body_feature_definitions,
    "body_feature_definitions",
    no_payload,
    read
);
engine_command!(engine_datum_plane_create, "datum_plane_create");
engine_command!(engine_datum_plane_edit, "datum_plane_edit");
engine_command!(engine_preview_segment, "preview_segment", read);
engine_command!(engine_eval_expression, "eval_expression", read);
engine_command!(engine_add_line, "add_line");
engine_command!(
    engine_preview_segment_locked,
    "preview_segment_locked",
    read
);
engine_command!(engine_add_line_locked, "add_line_locked");
engine_command!(engine_add_point, "add_point");
engine_command!(engine_add_line_midpoint, "add_line_midpoint");
engine_command!(engine_add_rectangle, "add_rectangle");
engine_command!(engine_add_rectangle_locked, "add_rectangle_locked");
engine_command!(engine_add_circle, "add_circle");
engine_command!(engine_add_circle_locked, "add_circle_locked");
engine_command!(engine_preview_creation, "preview_creation");
engine_command!(engine_add_slot, "add_slot");
engine_command!(engine_add_spline, "add_spline");
engine_command!(engine_add_arc_3pt, "add_arc_3pt");
engine_command!(engine_add_arc_center, "add_arc_center");
engine_command!(engine_add_constraint, "add_constraint");
engine_command!(engine_add_constraints, "add_constraints");
engine_command!(engine_add_dimension, "add_dimension");
engine_command!(engine_edit_dimension, "edit_dimension");
engine_command!(engine_set_dimension_mode, "set_dimension_mode");
engine_command!(engine_move_dimension, "move_dimension");
engine_command!(engine_delete_dimension, "delete_dimension");
engine_command!(engine_delete_constraint, "delete_constraint");
engine_command!(engine_set_dimension_style, "set_dimension_style");
engine_command!(engine_fillet_preview, "fillet_preview", read);
engine_command!(engine_fillet_lines, "fillet_lines");
engine_command!(engine_chamfer_lines, "chamfer_lines");
engine_command!(engine_offset_preview, "offset_preview", read);
engine_command!(engine_offset_curve, "offset_curve");
engine_command!(engine_trim_preview, "trim_preview", read);
engine_command!(engine_trim_entity, "trim_entity");
engine_command!(engine_extend_entity, "extend_entity");
engine_command!(engine_break_curve, "break_curve");
engine_command!(engine_mirror_entities, "mirror_entities");
engine_command!(engine_rectangular_pattern, "rectangular_pattern");
engine_command!(engine_circular_pattern, "circular_pattern");
engine_command!(engine_move_copy_entities, "move_copy_entities");
engine_command!(engine_scale_entities, "scale_entities");
engine_command!(engine_polygon_create, "polygon_create");
engine_command!(engine_toggle_fix, "toggle_fix");
engine_command!(engine_toggle_fix_entities, "toggle_fix_entities");
engine_command!(engine_move_point, "move_point");
engine_command!(engine_delete_entity, "delete_entity");
engine_command!(engine_delete_entities, "delete_entities");
engine_command!(engine_undo, "undo", no_payload);
engine_command!(engine_redo, "redo", no_payload);
engine_command!(engine_set_grid_snap, "set_grid_snap");
// Viewport zoom updates transient snap spacing, not the parametric model. Use
// the non-revision path so camera animation cannot invalidate a queued edit.
engine_command!(engine_set_grid_step, "set_grid_step", read);

#[tauri::command]
fn engine_solid_extrude(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_extrude(payload))
}

#[tauri::command]
fn engine_solid_edit_extrude(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_extrude(payload))
}

#[tauri::command]
fn engine_solid_revolve(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_revolve(payload))
}

#[tauri::command]
fn engine_solid_edit_revolve(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_revolve(payload))
}

#[tauri::command]
fn engine_solid_sweep(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_sweep(payload))
}

#[tauri::command]
fn engine_solid_edit_sweep(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_sweep(payload))
}

#[tauri::command]
fn engine_solid_loft(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_loft(payload))
}

#[tauri::command]
fn engine_solid_edit_loft(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_loft(payload))
}

#[tauri::command]
fn engine_solid_rib(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_rib(payload))
}

#[tauri::command]
fn engine_solid_edit_rib(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_rib(payload))
}

#[tauri::command]
fn engine_solid_fillet(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_fillet(payload))
}

#[tauri::command]
fn engine_solid_edit_fillet(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_fillet(payload))
}

#[tauri::command]
fn engine_solid_chamfer(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_chamfer(payload))
}

#[tauri::command]
fn engine_solid_edit_chamfer(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_chamfer(payload))
}

#[tauri::command]
fn engine_solid_hole(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_hole(payload))
}

#[tauri::command]
fn engine_solid_edit_hole(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_hole(payload))
}

#[tauri::command]
fn engine_solid_body_feature(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_body_feature(payload))
}

#[tauri::command]
fn engine_solid_edit_body_feature(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_edit_body_feature(payload))
}

#[tauri::command]
fn engine_solid_recompute(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_recompute())
}

#[tauri::command]
fn engine_solid_set_rollback(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_set_rollback(payload))
}

#[tauri::command]
fn engine_solid_delete_feature(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_delete_feature(payload))
}

#[tauri::command]
fn engine_solid_reorder_feature(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    bridge.run_ui_mutation(window.label(), || state.solid_reorder_feature(payload))
}

#[tauri::command]
fn engine_project_load(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    payload: &str,
) -> String {
    let started = std::time::Instant::now();
    let result =
        bridge.run_project_replacement(window.label(), &state, || state.project_load(payload));
    startup_metrics::record_document_span("nativeProjectLoad", started);
    result
}

#[tauri::command]
fn engine_project_new(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
) -> String {
    bridge.run_project_replacement(window.label(), &state, || state.project_new())
}

#[tauri::command]
fn engine_project_session_bind(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    viewport: tauri::State<'_, NativeViewport>,
    session_id: &str,
) -> String {
    bridge.with_project_session_transition(window.label(), &state, || {
        let result = state.bind_project_session(session_id);
        let succeeded = serde_json::from_str::<serde_json::Value>(&result)
            .ok()
            .and_then(|envelope| envelope.get("ok").and_then(serde_json::Value::as_bool))
            .unwrap_or(false);
        if succeeded {
            // Recovery can hydrate the bootstrap context before its frontend tab
            // id is known. Transfer the existing Bevy entities/cache rather than
            // deleting the solid faces while leaving their edge overlay behind.
            if let Err(error) = viewport
                .rebind_model_session(BOOTSTRAP_SESSION_ID.to_string(), session_id.to_string())
            {
                eprintln!("could not rebind bootstrap viewport session: {error}");
            }
        }
        result
    })
}

#[tauri::command]
fn engine_project_session_create(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    session_id: &str,
) -> String {
    bridge.with_project_session_transition(window.label(), &state, || {
        state.create_project_session(session_id)
    })
}

#[tauri::command]
fn engine_project_session_activate(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    session_id: &str,
) -> String {
    bridge.with_project_session_transition(window.label(), &state, || {
        state.activate_project_session(session_id)
    })
}

#[tauri::command]
fn engine_project_session_drop(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, session_bridge::SessionBridgeState>,
    state: tauri::State<'_, AppState>,
    viewport: tauri::State<'_, NativeViewport>,
    session_id: &str,
) -> String {
    let result = bridge.with_project_session_transition(window.label(), &state, || {
        state.drop_project_session(session_id)
    });
    let succeeded = serde_json::from_str::<serde_json::Value>(&result)
        .ok()
        .and_then(|envelope| envelope.get("ok").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    if succeeded {
        bridge.drop_bound_project_session(window.label(), session_id);
        let _ = viewport.drop_model_session(session_id.to_string());
    }
    result
}

#[tauri::command]
fn engine_export_step(state: tauri::State<'_, AppState>, payload: &str) -> Result<Vec<u8>, String> {
    state.export_step(payload)
}

#[tauri::command]
fn engine_export_stl(state: tauri::State<'_, AppState>, payload: &str) -> Result<Vec<u8>, String> {
    state.export_stl(payload)
}

#[tauri::command]
fn engine_export_3mf(state: tauri::State<'_, AppState>, payload: &str) -> Result<Vec<u8>, String> {
    state.export_3mf(payload)
}

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

fn cam_library_config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|error| format!("Could not resolve the per-user config directory: {error}"))
}

#[tauri::command]
async fn cam_posts_list(app: tauri::AppHandle) -> Result<cam_posts::Catalog, String> {
    let config = cam_library_config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || cam_posts::list(&config))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cam_posts_import(
    app: tauri::AppHandle,
    source: String,
) -> Result<cam_posts::Catalog, String> {
    let config = cam_library_config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || cam_posts::import(&config, Path::new(&source)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cam_posts_save_profile(
    app: tauri::AppHandle,
    file_name: String,
    machine: nbcad_cam::CamMachineAssignmentDto,
) -> Result<cam_posts::Catalog, String> {
    let config = cam_library_config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        cam_posts::save_profile(&config, &file_name, machine)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cam_posts_open_folder(app: tauri::AppHandle) -> Result<(), String> {
    let config = cam_library_config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = cam_posts::directory(&config)?;
        #[cfg(target_os = "macos")]
        let mut command = std::process::Command::new("open");
        #[cfg(target_os = "windows")]
        let mut command = std::process::Command::new("explorer");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let mut command = std::process::Command::new("xdg-open");
        command
            .arg(path)
            .spawn()
            .map_err(|e| format!("Could not open post folder: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cam_library_load(app: tauri::AppHandle) -> Result<cam_library::Snapshot, String> {
    let config = cam_library_config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || cam_library::load(&config))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cam_library_save(
    app: tauri::AppHandle,
    json: String,
    expected_path: String,
    expected_revision: String,
) -> Result<cam_library::Snapshot, String> {
    let config = cam_library_config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        cam_library::save(&config, &json, &expected_path, &expected_revision)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cam_library_location(app: tauri::AppHandle) -> Result<cam_library::Location, String> {
    let config = cam_library_config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || cam_library::get_location(&config))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cam_library_inspect_location(
    app: tauri::AppHandle,
    directory: Option<String>,
) -> Result<cam_library::Location, String> {
    let config = cam_library_config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        cam_library::inspect_location(&config, directory.as_deref().map(Path::new))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cam_library_set_location(
    app: tauri::AppHandle,
    directory: Option<String>,
    action: cam_library::LocationAction,
) -> Result<cam_library::Location, String> {
    let config = cam_library_config_dir(&app)?;
    let location = tauri::async_runtime::spawn_blocking(move || {
        cam_library::set_location(&config, directory.as_deref().map(Path::new), action)
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = app.emit("cam-library-location-changed", &location);
    Ok(location)
}

#[tauri::command]
fn read_binary_file(path: String) -> Result<Vec<u8>, String> {
    let started = std::time::Instant::now();
    let result = (|| {
        let metadata =
            fs::metadata(&path).map_err(|error| format!("could not read file: {error}"))?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err("file is larger than the 256 MB safety limit".to_string());
        }
        fs::read(path).map_err(|error| format!("could not read file: {error}"))
    })();
    startup_metrics::record_document_span("nativeFileRead", started);
    result
}

/// Write bytes to a path atomically (temp file + rename).
#[tauri::command]
fn write_binary_file_atomic(path: String, bytes: Vec<u8>) -> Result<(), String> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("file is larger than the 256 MB safety limit".to_string());
    }
    let target = PathBuf::from(path);
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "save path has no valid file name".to_string())?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("could not create temporary save file: {error}"))?;
        file.write_all(&bytes)
            .map_err(|error| format!("could not write save file: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("could not flush save file: {error}"))?;
        fs::rename(&temporary, &target)
            .map_err(|error| format!("could not replace save file: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub fn run() {
    run_with_launch_requests(launch_requests::LaunchRequestsState::default());
}

pub fn run_with_launch_requests(launch_requests: launch_requests::LaunchRequestsState) {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage({
            let started = std::time::Instant::now();
            let state = AppState::new();
            startup_metrics::record_native_span("cadEngineSetup", started);
            state
        })
        .manage(CamSimulationService::default())
        .manage(launch_requests)
        .manage(cam_playback::CamPlaybackService::default())
        .manage(NativeQuitState::default())
        .manage(scripts::NativeScriptState::default())
        .manage(recipe_links::RecipeLinkState::default())
        .manage(native_menu::NativeEditMenuState::default())
        .manage(native_menu::NativeFileMenuState::default())
        .manage(session_bridge::SessionBridgeState::default())
        .manage(SixDofMouseState::default())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                window
                    .state::<session_bridge::SessionBridgeState>()
                    .drop_window(window.label());
            }
        });
    #[cfg(target_os = "macos")]
    let builder = builder
        .menu(native_menu::build)
        .on_menu_event(native_menu::handle_event);
    builder
        .setup(|app| {
            startup_metrics::record_native_milestone("setupReached");
            app.state::<launch_requests::LaunchRequestsState>()
                .install(app.handle());
            recipe_links::install(app.handle());
            session_bridge::start_mcp_wake_loop(app.handle().clone());
            let viewport = NativeViewport::install(app).map_err(std::io::Error::other)?;
            let (
                session_id,
                geometry_revision,
                scene,
                active_sketch,
                finished_sketches,
                datum_planes,
                profile_catalog,
                body_appearances,
                body_poses,
                instance_body_poses,
            ) = app.state::<AppState>().viewport_snapshot();
            let _ = viewport.sync_model(ViewportModel {
                session_id,
                geometry_revision,
                scene,
                active_sketch,
                finished_sketches,
                datum_planes,
                profile_catalog,
                body_appearances,
                body_poses,
                instance_body_poses,
            });
            app.manage(viewport);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            system_memory_status,
            native_unsaved_set,
            native_force_quit,
            scripts::native_script_inspect,
            scripts::native_script_run,
            scripts::native_script_preview,
            scripts::native_script_preview_render,
            scripts::native_script_preview_open,
            scripts::native_script_preview_close,
            scripts::native_script_preview_release,
            scripts::native_script_examples,
            native_build_info,
            recipe_links::native_recipe_open_pending,
            recipe_links::native_recipe_open_ack,
            get_document,
            native_viewport_set_layout,
            native_viewport_set_suspended,
            native_viewport_sync_model,
            native_viewport_set_camera,
            native_viewport_set_preview,
            native_viewport_set_presentation,
            native_viewport_pick,
            native_viewport_metrics,
            native_viewport_start,
            launch_requests::native_launch_pending,
            launch_requests::native_launch_ack,
            launch_requests::native_new_window,
            native_window_prepare_workspace,
            native_window_set_locale,
            startup_metrics::startup_record,
            startup_metrics::startup_measurement_settings,
            native_menu::native_edit_menu_set_state,
            native_menu::native_file_menu_set_state,
            read_binary_file,
            write_binary_file_atomic,
            cam_library_load,
            cam_library_save,
            cam_library_location,
            cam_library_inspect_location,
            cam_library_set_location,
            cam_posts_list,
            cam_posts_import,
            cam_posts_save_profile,
            cam_posts_open_folder,
            session_bridge::mcp_session_bridge_reserve,
            session_bridge::mcp_session_bridge_write,
            session_bridge::mcp_session_bridge_control,
            session_bridge::mcp_window_control,
            session_bridge::mcp_path_exists,
            session_bridge::mcp_session_bridge_heartbeat,
            session_bridge::mcp_session_bridge_note_mutation,
            session_bridge::mcp_session_bridge_apply_inbox,
            six_dof_mouse::six_dof_mouse_devices,
            six_dof_mouse::six_dof_mouse_connect,
            six_dof_mouse::six_dof_mouse_disconnect,
            engine_begin_sketch,
            engine_document_set_name,
            engine_project_export_model,
            engine_project_new,
            engine_project_load,
            engine_project_session_bind,
            engine_project_session_create,
            engine_project_session_activate,
            engine_project_session_drop,
            engine_export_step,
            engine_export_stl,
            engine_export_3mf,
            engine_end_sketch,
            engine_finished_sketches,
            engine_edit_sketch,
            engine_active_sketch,
            engine_profile_catalog,
            engine_solid_scene,
            engine_body_appearances,
            engine_project_visibility,
            engine_project_set_visibility,
            engine_named_views,
            engine_upsert_named_view,
            engine_rename_named_view,
            engine_delete_named_view,
            engine_set_named_views,
            engine_recall_named_view,
            engine_clear_named_view,
            engine_construction_set_visibility,
            engine_drawing_document,
            engine_drawing_apply,
            engine_drawing_set_document,
            engine_assembly_document,
            engine_assembly_set_document,
            engine_assembly_solution,
            engine_assembly_create_component,
            engine_assembly_update_component,
            engine_assembly_create_occurrence,
            engine_assembly_update_occurrence,
            engine_assembly_duplicate_occurrence,
            engine_assembly_set_occurrence_grounded,
            engine_assembly_set_occurrence_pose,
            engine_assembly_preview_joint,
            engine_assembly_create_joint,
            engine_assembly_update_joint,
            engine_assembly_preview_joint_update,
            engine_assembly_delete_joint,
            engine_assembly_set_joint_enabled,
            engine_assembly_set_joint_motion,
            engine_assembly_preview_joint_motion,
            engine_assembly_set_grounded_body,
            engine_assembly_set_joint_coordinates,
            engine_assembly_preview_joint_coordinates,
            engine_assembly_preview_mechanism_drag,
            engine_assembly_apply_joint_motions,
            engine_assembly_create_position,
            engine_assembly_update_position,
            engine_assembly_delete_position,
            engine_assembly_apply_position,
            engine_assembly_create_motion_study,
            engine_assembly_update_motion_study,
            engine_assembly_delete_motion_study,
            engine_assembly_sample_motion_study,
            engine_assembly_export_motion_path_csv,
            engine_assembly_create_contact_set,
            engine_assembly_update_contact_set,
            engine_assembly_delete_contact_set,
            engine_assembly_interference_check,
            engine_assembly_evaluate_motion_study,
            engine_assembly_swept_collision_check,
            engine_drawing_projection,
            engine_drawing_export,
            engine_cam_document,
            engine_geometry_edge_chain,
            engine_cam_chamfer_geometry,
            engine_cam_cutter_mesh,
            engine_cam_set_document,
            engine_cam_toolpath_statuses,
            engine_cam_regenerate_operation,
            engine_cam_regenerate_setup,
            engine_cam_plan,
            engine_cam_post,
            engine_cam_analyze_nbpost,
            engine_cam_simulate,
            cam_playback::engine_cam_playback_open,
            cam_playback::engine_cam_playback_sample,
            cam_playback::engine_cam_playback_present,
            cam_playback::engine_cam_playback_close,
            engine_cam_simulate_gcode,
            engine_cam_post_events,
            engine_set_body_appearance,
            engine_extrude_definitions,
            engine_revolve_definitions,
            engine_sweep_definitions,
            engine_loft_definitions,
            engine_rib_definitions,
            engine_fillet_definitions,
            engine_chamfer_definitions,
            engine_hole_definitions,
            engine_datum_plane_definitions,
            engine_body_feature_definitions,
            engine_datum_plane_create,
            engine_datum_plane_edit,
            engine_preview_segment,
            engine_eval_expression,
            engine_add_line,
            engine_preview_segment_locked,
            engine_add_line_locked,
            engine_add_point,
            engine_add_line_midpoint,
            engine_add_rectangle,
            engine_add_rectangle_locked,
            engine_add_circle,
            engine_add_circle_locked,
            engine_preview_creation,
            engine_add_slot,
            engine_add_spline,
            engine_add_arc_3pt,
            engine_add_arc_center,
            engine_add_constraint,
            engine_add_constraints,
            engine_add_dimension,
            engine_edit_dimension,
            engine_set_dimension_mode,
            engine_move_dimension,
            engine_delete_dimension,
            engine_delete_constraint,
            engine_set_dimension_style,
            engine_fillet_preview,
            engine_fillet_lines,
            engine_chamfer_lines,
            engine_offset_preview,
            engine_offset_curve,
            engine_trim_preview,
            engine_trim_entity,
            engine_extend_entity,
            engine_break_curve,
            engine_mirror_entities,
            engine_rectangular_pattern,
            engine_circular_pattern,
            engine_move_copy_entities,
            engine_scale_entities,
            engine_polygon_create,
            engine_toggle_fix,
            engine_toggle_fix_entities,
            engine_move_point,
            engine_delete_entity,
            engine_delete_entities,
            engine_undo,
            engine_redo,
            engine_set_grid_snap,
            engine_set_grid_step,
            engine_solid_extrude,
            engine_solid_edit_extrude,
            engine_solid_revolve,
            engine_solid_edit_revolve,
            engine_solid_sweep,
            engine_solid_edit_sweep,
            engine_solid_loft,
            engine_solid_edit_loft,
            engine_solid_rib,
            engine_solid_edit_rib,
            engine_solid_fillet,
            engine_solid_edit_fillet,
            engine_solid_chamfer,
            engine_solid_edit_chamfer,
            engine_solid_hole,
            engine_solid_edit_hole,
            engine_solid_body_feature,
            engine_solid_edit_body_feature,
            engine_solid_recompute,
            engine_solid_set_rollback,
            engine_solid_delete_feature,
            engine_solid_reorder_feature,
        ])
        .build(tauri::generate_context!())
        .expect("error while building noBS CAD")
        .run(|app, event| {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if let tauri::RunEvent::Opened { urls } = &event {
                let targets = urls
                    .iter()
                    .filter_map(|url| {
                        if url.scheme() == "file" {
                            url.to_file_path().ok().and_then(|path| {
                                path.to_str()
                                    .map(|path| launch_requests::LaunchTarget::File {
                                        path: path.into(),
                                    })
                            })
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if !targets.is_empty() {
                    if let Err(error) = launch_requests::enqueue(app, &targets) {
                        eprintln!("Launch request rejected: {error}");
                    }
                }
            }
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let state = app.state::<NativeQuitState>();
                if state.approved.swap(false, Ordering::AcqRel) {
                    return;
                }
                if state.unsaved.load(Ordering::Acquire) {
                    api.prevent_exit();
                    let _ = app.emit("native-quit-request", ());
                }
            }
        });
}
