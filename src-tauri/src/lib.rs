//! noBS CAD native shell (Tauri 2).
//!
//! Thin IPC layer over the engine crates (`nbcad-core` /
//! `nbcad-sketch`): the shell owns the [`state::AppState`] and exposes
//! the engine API as JSON-string commands. Every `engine_*` command
//! dispatches through `nbcad_sketch::host::handle` — the exact code
//! path the WASM host uses, so native and browser behavior are identical.
//! All modeling logic lives in the engine crates, never here.

mod native_menu;
pub mod native_viewport;
mod scripts;
mod session_bridge;
mod six_dof_mouse;
mod state;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use native_viewport::{
    NativePick, NativePickPurpose, NativeViewport, NativeViewportMetrics, ViewportCamera,
    ViewportLayout, ViewportModel, ViewportPresentation, ViewportPreview,
};
use nbcad_core::DocumentDto;
use serde::Serialize;
use six_dof_mouse::SixDofMouseState;
use state::{AppState, BOOTSTRAP_SESSION_ID};
use tauri::{Emitter, Manager};

/// Health-check command used by the frontend IPC wrapper.
#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemMemoryStatus {
    total_bytes: u64,
    available_bytes: u64,
    pressure: &'static str,
}

/// Native authority for the application-wide unsaved-work guard. Window
/// close requests are handled by the frontend so it can show the platform
/// confirmation dialog; `ExitRequested` additionally covers application-menu
/// Quit and Cmd/Ctrl+Q, which can bypass a webview's close callback.
#[derive(Default)]
struct NativeQuitState {
    unsaved: AtomicBool,
    approved: AtomicBool,
}

#[tauri::command]
fn native_unsaved_set(state: tauri::State<'_, NativeQuitState>, unsaved: bool) {
    state.unsaved.store(unsaved, Ordering::Release);
}

#[tauri::command]
fn native_force_quit(app: tauri::AppHandle, state: tauri::State<'_, NativeQuitState>) {
    state.approved.store(true, Ordering::Release);
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
async fn native_viewport_sync_model(
    engine: tauri::State<'_, AppState>,
    viewport: tauri::State<'_, NativeViewport>,
) -> Result<(), String> {
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
    })
}

#[tauri::command]
fn native_viewport_set_camera(
    viewport: tauri::State<'_, NativeViewport>,
    camera: ViewportCamera,
) -> Result<(), String> {
    viewport.set_camera(camera)
}

#[tauri::command]
fn native_viewport_set_preview(
    viewport: tauri::State<'_, NativeViewport>,
    preview: ViewportPreview,
) -> Result<(), String> {
    viewport.set_preview(preview)
}

#[tauri::command]
fn native_viewport_set_presentation(
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
    bridge.run_project_replacement(window.label(), &state, || state.project_load(payload))
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

#[tauri::command]
fn read_binary_file(path: String) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(&path).map_err(|error| format!("could not read file: {error}"))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err("file is larger than the 256 MB safety limit".to_string());
    }
    fs::read(path).map_err(|error| format!("could not read file: {error}"))
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
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .manage(NativeQuitState::default())
        .manage(scripts::NativeScriptState::default())
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
            get_document,
            native_viewport_set_layout,
            native_viewport_set_suspended,
            native_viewport_sync_model,
            native_viewport_set_camera,
            native_viewport_set_preview,
            native_viewport_set_presentation,
            native_viewport_pick,
            native_viewport_metrics,
            native_menu::native_edit_menu_set_state,
            native_menu::native_file_menu_set_state,
            read_binary_file,
            write_binary_file_atomic,
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
