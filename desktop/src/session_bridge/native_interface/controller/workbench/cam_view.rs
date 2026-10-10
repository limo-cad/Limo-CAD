//! Cached native presentation of the shared CAM planner and stock simulator.
//! Jobs consume immutable document snapshots; their results cannot cross an
//! owner/revision/selection boundary. They never mutate machining intent.
use super::*;
use crate::native_viewport::{
    self, ViewportCamStock, ViewportCamTool, ViewportLineLayer, ViewportPresentation,
    ViewportPreview,
};
use limo_cad_cam::{
    CamDocumentDto, CamResolvedStockDto, CamSetupDto, CamSimulationCancellation,
    CamSimulationRequestDto, CamSimulationResultDto, CamSimulationTargetDto, CamStockMeshDto,
};
use std::sync::{mpsc, Arc};

mod geometry;
pub(crate) mod nc_dialog;
mod nc_input;
mod nc_prepare;
mod playback;
mod report;
pub(crate) mod settings;
#[cfg(test)]
mod tests;
mod timeline;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum View {
    Model,
    #[default]
    Stock,
    Compare,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    View(View),
    Paths,
    Simulate,
    Cancel,
    Play,
    Start,
    Back,
    Forward,
    End,
    Speed,
    Seek,
    Report,
    ReportPage(i32),
    CloseReport,
    Settings,
    CloseSettings,
    Detail,
    Tolerance,
    Nc(u64, nc_dialog::Command),
}

#[derive(Clone, PartialEq, Eq)]
struct Key {
    owner: DocumentContext,
    revision: u64,
    selection: Option<cam::Selection>,
}
struct Prepared {
    paths: Vec<ViewportLineLayer>,
    tool: Option<ViewportCamTool>,
    simulation: Option<CamSimulationResultDto>,
    stock: Option<ViewportCamStock>,
    message: String,
    details: String,
    path_id: u64,
    start_time: f64,
    nc_kernel: Option<Mutex<limo_cad_cam::CamPlayback>>,
}
struct Pending {
    key: Key,
    generation: u64,
    cancellation: CamSimulationCancellation,
    receiver: Mutex<mpsc::Receiver<Result<Prepared, String>>>,
}
struct Applied {
    owner: DocumentContext,
    preview_revision: u64,
    before_preview: Arc<ViewportPreview>,
    before_presentation: ViewportPresentation,
    after_presentation: ViewportPresentation,
    before_stock: Option<ViewportCamStock>,
    stock_revision: u64,
    playback_stock_revision: Option<u64>,
}
#[derive(Resource, Default)]
struct State {
    key: Option<Key>,
    document: Option<Arc<CamDocumentDto>>,
    setup: Option<u64>,
    operation: Option<u64>,
    warning: Option<String>,
    pending: Option<Pending>,
    prepared: Option<Prepared>,
    player: Option<playback::Player>,
    playback_action: Option<Command>,
    seek_target: Option<f64>,
    report: bool,
    report_page: usize,
    settings: settings::Settings,
    settings_open: bool,
    generation: u64,
    simulation_requested: bool,
    nc_input: Option<nc_input::Input>,
    request_pending: bool,
    dirty: bool,
    view: View,
    paths: bool,
    error: String,
    applied: Option<Applied>,
    widgets: Widgets,
}

pub(crate) fn caption(world: &World) -> Option<String> {
    let state = world.get_resource::<State>()?;
    state.key.as_ref()?;
    let entity = state.widgets.entity("cam-view-status")?;
    world.get::<Text>(entity).map(|text| text.0.clone())
}

pub(crate) fn modal(world: &World) -> Option<&'static str> {
    if nc_dialog::modal(world) {
        return Some("cam-nc-source");
    }
    let state = world.get_resource::<State>()?;
    if state.settings_open {
        Some("cam-simulation-settings")
    } else {
        state.report.then_some("cam-report")
    }
}
pub(crate) fn report_caption(world: &World) -> Option<String> {
    world
        .get_resource::<State>()
        .filter(|s| s.report)
        .and_then(|s| {
            if !s.error.is_empty() {
                Some(s.error.clone())
            } else {
                s.prepared.as_ref().map(|p| p.details.clone())
            }
        })
}
pub(crate) fn escape(world: &mut World) {
    if nc_dialog::modal(world) {
        nc_dialog::close(world);
        return;
    }
    if let Some(mut state) = world.get_resource_mut::<State>() {
        state.report = false;
        state.settings_open = false;
    }
}

fn request_nc(world: &mut World, input: nc_input::Input) -> Result<(), String> {
    let mut state = world
        .get_resource_mut::<State>()
        .ok_or("Open the CAM workspace")?;
    if state.setup.is_none() || state.key.is_none() {
        return Err("Choose a CAM setup".into());
    }
    if state.pending.is_some() {
        return Err("Wait for the current preview or cancel it".into());
    }
    state.generation = state.generation.wrapping_add(1);
    state.nc_input = Some(input);
    state.simulation_requested = true;
    state.player = None;
    state.prepared = None;
    state.playback_action = None;
    state.seek_target = None;
    state.request_pending = true;
    state.report = false;
    state.error.clear();
    state.dirty = true;
    Ok(())
}

pub(super) fn execute(world: &mut World, command: &Command) -> Result<Value, String> {
    if cam::geometry_pick::active(world) {
        return Err("Finish geometry picking before changing the CAM preview".into());
    }
    if matches!(
        command,
        Command::Simulate
            | Command::Play
            | Command::Start
            | Command::Back
            | Command::Forward
            | Command::End
    ) && cam::editing_dirty(world)
    {
        return Err("Apply or cancel the CAM edits before simulating".into());
    }
    let mut state = world
        .get_resource_mut::<State>()
        .ok_or("Open the CAM workspace")?;
    match command {
        Command::View(view) => state.view = *view,
        Command::Paths => state.paths = !state.paths,
        Command::Simulate => {
            if state.setup.is_none() {
                return Err("Choose a setup to simulate".into());
            }
            if state.pending.is_some() {
                return Err("Wait for the current CAM preview or cancel it".into());
            }
            state.generation = state.generation.wrapping_add(1);
            state.simulation_requested = true;
            state.nc_input = None;
            state.prepared = None;
            state.player = None;
            state.playback_action = None;
            state.request_pending = true;
            state.error.clear();
        }
        Command::Play
        | Command::Start
        | Command::Back
        | Command::Forward
        | Command::End
        | Command::Speed => {
            if state
                .prepared
                .as_ref()
                .and_then(|p| p.simulation.as_ref())
                .is_none()
            {
                return Err("Simulate the applied CAM document before playback".into());
            }
            state.playback_action = Some(command.clone());
        }
        Command::Cancel => {
            if let Some(pending) = &state.pending {
                pending.cancellation.cancel();
            }
            state.generation = state.generation.wrapping_add(1);
            state.request_pending = false;
            state.simulation_requested = false;
            state.error.clear();
            state.player = None;
            state.playback_action = None;
            if state.nc_input.is_some() {
                state.prepared = None;
            }
            state.nc_input = None;
        }
        Command::Seek => return Err("Use the playback timeline to seek".into()),
        Command::Report => {
            if let Some(player) = state.player.as_mut().filter(|p| p.playing) {
                player.playing = false;
                player.ticket = player.ticket.wrapping_add(1);
                player.requested = None;
            }
            state.report = true;
            state.settings_open = false;
            state.report_page = 0;
        }
        Command::ReportPage(delta) => {
            state.report_page = state.report_page.saturating_add_signed(*delta as isize)
        }
        Command::CloseReport => state.report = false,
        Command::Settings => {
            if let Some(player) = state.player.as_mut().filter(|p| p.playing) {
                player.playing = false;
                player.ticket = player.ticket.wrapping_add(1);
                player.requested = None;
            }
            state.report = false;
            state.settings_open = true;
        }
        Command::CloseSettings => state.settings_open = false,
        Command::Detail | Command::Tolerance => {
            return Err("Set the simulation preference's value".into())
        }
        Command::Nc(..) => return Err("Use the current NC input controls".into()),
    }
    state.dirty = true;
    Ok(json!({"handled":true}))
}

pub(crate) fn seek(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
    input: &limo_cad_interface::ControlInput,
) -> Result<Value, String> {
    if cam::editing_dirty(world) || cam::geometry_pick::active(world) {
        return Err("Apply or cancel CAM edits before playback".into());
    }
    let limo_cad_interface::ControlInput::SetValue(value) = input else {
        return Err("Use the playback timeline to seek".into());
    };
    let time: f64 = value.parse().map_err(|_| "Playback time must be numeric")?;
    let mut state = world
        .get_resource_mut::<State>()
        .ok_or("Simulate before playback")?;
    let key = state.key.as_ref().ok_or("CAM view changed")?;
    if &key.owner != owner || key.revision != revision {
        return Err("CAM document changed before seeking".into());
    }
    let prepared = state.prepared.as_ref().ok_or("Simulate before playback")?;
    let simulation = prepared
        .simulation
        .as_ref()
        .ok_or("Simulate before playback")?;
    if !time.is_finite() || time < prepared.start_time || time > simulation.estimated_seconds {
        return Err("Playback time is outside this simulation".into());
    }
    state.seek_target = Some(time);
    state.playback_action = Some(Command::Seek);
    Ok(json!({"handled":true}))
}

fn selection(
    document: &CamDocumentDto,
    selected: Option<cam::Selection>,
) -> (Option<u64>, Option<u64>) {
    match selected {
        Some(cam::Selection::Setup(id)) => (document.setup(id).map(|s| s.id), None),
        Some(cam::Selection::Operation(id)) => (
            document
                .setups
                .iter()
                .find(|s| s.operations.iter().any(|op| op.id() == id))
                .map(|s| s.id),
            Some(id),
        ),
        _ => (
            document
                .active_setup_id
                .or_else(|| document.setups.first().map(|s| s.id)),
            None,
        ),
    }
}

fn simulation_request(
    world: &World,
    document: &CamDocumentDto,
    setup: &CamSetupDto,
    operation: Option<u64>,
    settings: settings::Settings,
) -> Result<CamSimulationRequestDto, String> {
    let scene = native_viewport::interface_geometry(world).scene;
    let mesh = |id| -> Result<CamStockMeshDto, String> {
        let body = scene
            .bodies
            .iter()
            .find(|b| b.id.0 == id)
            .ok_or("CAM body was removed")?;
        Ok(CamStockMeshDto {
            positions: body.mesh.positions.iter().map(|p| f64::from(*p)).collect(),
            indices: body.mesh.indices.clone(),
        })
    };
    let stock_id = stock_body(document, setup)?;
    let meshes = setup
        .body_ids
        .iter()
        .filter(|id| Some(id.0) != stock_id)
        .map(|id| mesh(id.0))
        .collect::<Result<Vec<_>, _>>()?;
    let mut request = CamSimulationRequestDto {
        setup_id: setup.id,
        voxel_size: None,
        max_voxels: None,
        stock_mesh: stock_id.map(mesh).transpose()?,
        target: (!meshes.is_empty()).then_some(CamSimulationTargetDto {
            cache_key: None,
            meshes,
            tolerance_mm: 0.1,
        }),
        through_operation_id: operation,
        completed_steps: None,
        playback_time_seconds: None,
    };
    settings.apply(setup, &mut request);
    Ok(request)
}

fn stock_body(document: &CamDocumentDto, setup: &CamSetupDto) -> Result<Option<u64>, String> {
    let mut current = setup;
    for _ in 0..=document.setups.len() {
        match current.resolved_stock {
            CamResolvedStockDto::ModelBody { body_id } => return Ok(Some(body_id)),
            CamResolvedStockDto::Rest { source_setup_id } => {
                current = document
                    .setup(source_setup_id)
                    .ok_or("Rest-stock source was removed")?;
            }
            _ => return Ok(None),
        }
    }
    Err("Rest-stock sources contain a cycle".into())
}

fn prepare(
    document: &CamDocumentDto,
    setup_id: u64,
    operation: Option<u64>,
    request: Option<CamSimulationRequestDto>,
    cancellation: &CamSimulationCancellation,
    warning: Option<String>,
) -> Result<Prepared, String> {
    let setup = document.setup(setup_id).ok_or("CAM setup was removed")?;
    if request.is_none()
        && !setup
            .operations
            .iter()
            .any(limo_cad_cam::CamOperationDto::enabled)
    {
        let message =
            "No enabled generated toolpaths. Add an operation or simulate NC.".to_string();
        return Ok(Prepared {
            paths: Vec::new(),
            tool: None,
            simulation: None,
            stock: None,
            details: message.clone(),
            message,
            path_id: 0,
            start_time: 0.,
            nc_kernel: None,
        });
    }
    let program = match operation {
        Some(operation) => limo_cad_cam::plan_setup_through(document, setup_id, operation),
        None => limo_cad_cam::plan_setup(document, setup_id),
    }
    .map_err(|e| e.to_string())?;
    let (mut paths, mut tool) = geometry::paths(document, setup, &program, operation)?;
    let mut warnings = program.warnings.clone();
    if let Some(warning) = warning {
        warnings.insert(0, warning);
    }
    let mut simulation = request
        .map(|request| {
            limo_cad_cam::simulate_setup_with_cancellation(document, &request, Some(cancellation))
                .map_err(|e| e.to_string())
        })
        .transpose()?;
    let stock = simulation.as_ref().and_then(crate::retained_cam_stock);
    static NEXT_PATH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let path_id = NEXT_PATH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut start_time = 0.;
    if let Some(result) = &simulation {
        let first_command = operation.and_then(|id| program.commands.iter().position(|command| {
            matches!(command, limo_cad_cam::CamCommandDto::SectionStart { operation_id, .. } if *operation_id == id)
        })).unwrap_or(0);
        start_time = result
            .steps
            .iter()
            .find(|step| step.command_index >= first_command)
            .map_or(0., |step| step.cumulative_seconds - step.duration_seconds);
        paths = timeline::paths(result, path_id, first_command)?;
        tool = timeline::pose(document, result, path_id, result.estimated_seconds)?.0;
    }
    let message = if let Some(result) = &mut simulation {
        result.stock_mesh = None;
        result.native_stock_present = stock.is_some();
        warnings.extend(result.warnings.iter().cloned());
        format!(
            "Simulation: {:.1} s · {} contacts · {:.1} mm³ removed · voxel {:.3} mm{}",
            result.estimated_seconds,
            result.collisions.len(),
            result.removed_volume_mm3,
            result.cell_size[0],
            result
                .comparison
                .as_ref()
                .map_or(String::new(), |c| format!(
                    " · gouge {:.1} mm³",
                    c.gouged_volume_mm3
                ))
        )
    } else {
        format!(
            "Toolpath preview · {} operations · {:.1} s",
            program.stats.operation_count, program.stats.estimated_seconds
        )
    };
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|warning| seen.insert(warning.clone()));
    let mut details = format!("{message}\n");
    for (index, warning) in warnings.iter().enumerate() {
        details.push_str(&format!("\nWarning {}: {warning}\n", index + 1));
    }
    if let Some(simulation) = &simulation {
        if let Some(comparison) = &simulation.comparison {
            details.push_str(&format!("\nTarget comparison\nRequested tolerance: {:.3} mm\nEffective voxel tolerance: {:.3} mm\nExcess material: {:.3} mm³\nGouged target: {:.3} mm³\nInitial stock shortfall: {:.3} mm³\n", comparison.requested_tolerance_mm, comparison.effective_tolerance_mm, comparison.excess_volume_mm3, comparison.gouged_volume_mm3, comparison.initial_shortfall_volume_mm3));
        }
        for (index, collision) in simulation.collisions.iter().enumerate() {
            details.push_str(&format!(
                "\nContact {} · command {} · setup X {:.3}, Y {:.3}, Z {:.3} mm\n{}\n",
                index + 1,
                collision.command_index,
                collision.position.x,
                collision.position.y,
                collision.position.z,
                collision.message
            ));
        }
    }
    let message = if warnings.is_empty() {
        message
    } else {
        format!(
            "{message} · {} warnings · Open Report for details",
            warnings.len()
        )
    };
    Ok(Prepared {
        paths,
        tool,
        simulation,
        stock,
        message,
        details,
        path_id,
        start_time,
        nc_kernel: None,
    })
}

fn restore(world: &mut World, services: &NativeServices, state: &mut State) -> Result<(), String> {
    let Some(applied) = state.applied.take() else {
        return Ok(());
    };
    let current_owner = services
        .bridge
        .native_document_context(&applied.owner.window_id, &services.engine)?;
    if current_owner != applied.owner {
        return Ok(());
    }
    services
        .bridge
        .with_native_document_owner(&services.engine, &applied.owner, || {
            let (session, _, mut presentation, _) = native_viewport::interface_view_snapshot(world);
            if session != applied.owner.document_id {
                return Ok(());
            }
            if native_viewport::interface_preview_revision(world) == applied.preview_revision {
                native_viewport::apply_interface_preview(world, &session, applied.before_preview)?;
            }
            if presentation.hidden_body_ids == applied.after_presentation.hidden_body_ids {
                presentation.hidden_body_ids = applied.before_presentation.hidden_body_ids;
            }
            if presentation.ghosted_body_ids == applied.after_presentation.ghosted_body_ids {
                presentation.ghosted_body_ids = applied.before_presentation.ghosted_body_ids;
            }
            if presentation.cam_tool == applied.after_presentation.cam_tool {
                presentation.cam_tool = applied.before_presentation.cam_tool;
            }
            if presentation.cam_tool_hidden == applied.after_presentation.cam_tool_hidden {
                presentation.cam_tool_hidden = applied.before_presentation.cam_tool_hidden;
            }
            let (stock_revision, _) = native_viewport::interface_cam_stock_snapshot(world);
            if stock_revision == applied.stock_revision {
                if presentation.cam_stock_visible == applied.after_presentation.cam_stock_visible {
                    presentation.cam_stock_visible = applied.before_presentation.cam_stock_visible;
                }
                native_viewport::apply_interface_cam_stock(world, &session, applied.before_stock)?;
            }
            if presentation.cam_path_progress == applied.after_presentation.cam_path_progress {
                presentation.cam_path_progress = applied.before_presentation.cam_path_progress;
            }
            native_viewport::apply_interface_view(world, &session, None, Some(presentation))
        })
}

fn display(world: &mut World, services: &NativeServices, state: &mut State) -> Result<(), String> {
    restore(world, services, state)?;
    let Some(key) = &state.key else { return Ok(()) };
    let Some(setup) = state
        .document
        .as_ref()
        .and_then(|d| state.setup.and_then(|id| d.setup(id)))
    else {
        return Ok(());
    };
    let picking = cam::geometry_pick::overlay(world);
    let is_picking = picking.is_some();
    let view = if is_picking { View::Model } else { state.view };
    let simulation = state.prepared.as_ref().and_then(|p| p.simulation.as_ref());
    let mut preview = geometry::stock(setup, simulation.is_none() && view != View::Model);
    if view == View::Model {
        preview.lines.clear();
    }
    if !is_picking {
        if let Some(prepared) = &state.prepared {
            preview
                .lines
                .extend(prepared.paths.iter().cloned().map(|mut line| {
                    line.hidden = !state.paths;
                    line
                }));
        }
    }
    if view != View::Model {
        if let Some(simulation) = simulation {
            let contacts: Vec<f32> = simulation
                .collisions
                .iter()
                .filter(|c| {
                    c.kind == limo_cad_cam::CamSimulationCollisionKindDto::RapidStockContact
                })
                .flat_map(|c| geometry::model_point(c.position, simulation.wcs))
                .collect();
            if !contacts.is_empty() {
                let span = (setup.stock.max.x - setup.stock.min.x)
                    .max(setup.stock.max.y - setup.stock.min.y);
                preview
                    .points
                    .push(crate::native_viewport::ViewportPointLayer {
                        color: [0.94, 0.67, 0.29, 0.95],
                        radius: (span as f32 * 0.003).clamp(0.08, 1.2),
                        positions: contacts.into(),
                        ..default()
                    });
            }
        }
    }
    if let Some(picking) = picking {
        preview = picking;
    }
    services
        .bridge
        .with_native_document_receipt(&services.engine, &key.owner, |revision| {
            if revision != key.revision {
                return Err("CAM document changed during preview preparation".into());
            }
            let (session, _, before, _) = native_viewport::interface_view_snapshot(world);
            if session != key.owner.document_id {
                return Err("CAM viewport document changed".into());
            }
            let before_preview = native_viewport::interface_preview_snapshot(world);
            let (_, before_stock) = native_viewport::interface_cam_stock_snapshot(world);
            let mut presentation = before.clone();
            if is_picking {
                presentation.hovered_body_id = None;
                presentation.hovered_occurrence_id = None;
                presentation.hovered_face_id = None;
                presentation.hovered_edge_id = None;
            }
            presentation.cam_tool_hidden = !state.paths;
            presentation.cam_tool = (state.paths && !is_picking)
                .then(|| state.prepared.as_ref().and_then(|p| p.tool))
                .flatten();
            presentation.cam_path_progress = None;
            if !is_picking {
                if let Some(frame) = state.player.as_ref().and_then(|p| p.frame.as_ref()) {
                    if let Some(prepared) = state.prepared.as_ref() {
                        let (tool, progress) = timeline::pose(
                            state.document.as_ref().unwrap(),
                            prepared.simulation.as_ref().unwrap(),
                            prepared.path_id,
                            frame.time,
                        )?;
                        presentation.cam_tool = tool;
                        presentation.cam_path_progress = progress;
                    }
                }
            }
            presentation.cam_stock_visible = view != View::Model && simulation.is_some();
            if presentation.cam_stock_visible {
                for id in &setup.body_ids {
                    let list = if view == View::Compare {
                        &mut presentation.ghosted_body_ids
                    } else {
                        &mut presentation.hidden_body_ids
                    };
                    if !list.contains(&id.0) {
                        list.push(id.0);
                    }
                }
            }
            if let Some(body_id) = stock_body(state.document.as_ref().unwrap(), setup)? {
                if (view == View::Model || presentation.cam_stock_visible)
                    && !presentation.hidden_body_ids.contains(&body_id)
                {
                    presentation.hidden_body_ids.push(body_id);
                }
            }
            native_viewport::apply_interface_preview(world, &session, preview)?;
            native_viewport::apply_interface_cam_stock(
                world,
                &session,
                state
                    .player
                    .as_ref()
                    .and_then(|p| p.frame.as_ref())
                    .map_or_else(
                        || state.prepared.as_ref().and_then(|p| p.stock.clone()),
                        |f| f.stock.clone(),
                    ),
            )?;
            native_viewport::apply_interface_view(
                world,
                &session,
                None,
                Some(presentation.clone()),
            )?;
            state.applied = Some(Applied {
                owner: key.owner.clone(),
                preview_revision: native_viewport::interface_preview_revision(world),
                before_preview,
                before_presentation: before,
                after_presentation: presentation,
                before_stock,
                stock_revision: native_viewport::interface_cam_stock_snapshot(world).0,
                playback_stock_revision: state
                    .player
                    .as_ref()
                    .and_then(|p| p.frame.as_ref())
                    .map(|f| f.stock_revision),
            });
            Ok(())
        })
}

/// Picking is composed by the existing preview owner, including its restore
/// receipt. It cannot become another writer of the same viewport buffers.
pub(super) fn geometry_hidden_bodies(world: &World) -> Result<Vec<u64>, String> {
    let (owner, _) = native_viewport::interface_camera_snapshot(world);
    let (_, presentation) = native_viewport::interface_navigation_source(world);
    if presentation.hidden_body_ids.len() > native_viewport::physical_pick::MAX_INSTANCES {
        return Err(
            "Viewport picking exceeds its visibility budget; use the geometry fields".into(),
        );
    }
    let mut hidden = presentation.hidden_body_ids.clone();
    if let Some(state) = world.get_resource::<State>() {
        if let Some(applied) = state
            .applied
            .as_ref()
            .filter(|applied| applied.owner.document_id == owner)
        {
            if hidden == applied.after_presentation.hidden_body_ids {
                if applied.before_presentation.hidden_body_ids.len()
                    > native_viewport::physical_pick::MAX_INSTANCES
                {
                    return Err(
                        "Viewport picking exceeds its visibility budget; use the geometry fields"
                            .into(),
                    );
                }
                hidden.clone_from(&applied.before_presentation.hidden_body_ids);
            }
        }
        if let Some(document) = state.document.as_ref() {
            if let Some(setup) = state.setup.and_then(|id| document.setup(id)) {
                if let Some(body) = stock_body(document, setup)? {
                    if !hidden.contains(&body) {
                        hidden.push(body);
                    }
                }
            }
        }
    }
    Ok(hidden)
}

pub(super) fn geometry_changed(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<State>() {
        state.dirty = true;
        if let Some(player) = &mut state.player {
            if player.playing {
                player.playing = false;
                player.ticket = player.ticket.wrapping_add(1);
                player.requested = None;
            }
        }
        state.playback_action = None;
    }
}

fn advance_playback(world: &World, state: &mut State) -> Result<bool, String> {
    if let Some(action) = state.playback_action.take() {
        let prepared = state.prepared.as_mut().ok_or("Simulate before playback")?;
        let simulation = prepared
            .simulation
            .as_ref()
            .ok_or("Simulate before playback")?;
        let duration = simulation.estimated_seconds;
        let start = prepared.start_time;
        if state.player.is_none() {
            let wake = world.get_resource::<NativeInterfaceHandle>().cloned();
            let mut player = if state.nc_input.is_some() {
                playback::Player::from_prepared(
                    prepared
                        .nc_kernel
                        .take()
                        .ok_or("Rebuild the NC simulation before restarting playback")?
                        .into_inner()
                        .map_err(|_| "NC playback ownership was poisoned")?,
                    wake,
                )?
            } else {
                let document = state.document.as_ref().ok_or("CAM document changed")?;
                let setup = document
                    .setup(state.setup.ok_or("Choose a setup")?)
                    .ok_or("CAM setup changed")?;
                let request =
                    simulation_request(world, document, setup, state.operation, state.settings)?;
                // The simulation kernel owns mutable playback state; its document
                // copy is intentional, unlike read-only preview preparation.
                playback::Player::new(document.as_ref().clone(), request, wake)?
            };
            player.seek(start, duration);
            state.player = Some(player);
        }
        let player = state.player.as_mut().unwrap();
        match action {
            Command::Play => player.toggle(start, duration),
            Command::Start => player.seek(start, duration),
            Command::End => player.seek(duration, duration),
            Command::Back => player.seek(
                timeline::adjacent_move(simulation, player.time(), false, start),
                duration,
            ),
            Command::Forward => player.seek(
                timeline::adjacent_move(simulation, player.time(), true, start),
                duration,
            ),
            Command::Speed => {
                player.speed = match player.speed {
                    0.25 => 0.5,
                    0.5 => 1.,
                    1. => 2.,
                    2. => 5.,
                    5. => 10.,
                    _ => 0.25,
                }
            }
            Command::Seek => player.seek(
                state.seek_target.take().ok_or("Choose a playback time")?,
                duration,
            ),
            _ => unreachable!(),
        }
    }
    let Some(player) = state.player.as_mut() else {
        return Ok(false);
    };
    let duration = state
        .prepared
        .as_ref()
        .and_then(|p| p.simulation.as_ref())
        .ok_or("Simulation changed")?
        .estimated_seconds;
    match player.poll(duration) {
        Ok(changed) => Ok(changed),
        Err(error) => {
            state.player = None;
            state.error = error;
            state.dirty = true;
            Ok(false)
        }
    }
}

/// Update only the retained stock and lightweight pose. Static paths are not
/// cloned or rebuilt on the frame clock, and every publication uses the same
/// exact owner/revision fence as the static preview.
fn display_frame(
    world: &mut World,
    services: &NativeServices,
    state: &mut State,
) -> Result<(), String> {
    let (Some(key), Some(prepared), Some(applied), Some(frame)) = (
        state.key.as_ref(),
        state.prepared.as_ref(),
        state.applied.as_mut(),
        state.player.as_ref().and_then(|p| p.frame.as_ref()),
    ) else {
        return Ok(());
    };
    services
        .bridge
        .with_native_document_receipt(&services.engine, &key.owner, |revision| {
            if revision != key.revision {
                return Err("CAM document changed during playback".into());
            }
            let (session, _, mut presentation, _) = native_viewport::interface_view_snapshot(world);
            let (stock_revision, _) = native_viewport::interface_cam_stock_snapshot(world);
            if session != key.owner.document_id
                || native_viewport::interface_preview_revision(world) != applied.preview_revision
                || stock_revision != applied.stock_revision
            {
                return Err("CAM presentation changed during playback".into());
            }
            let (tool, progress) = {
                timeline::pose(
                    state.document.as_ref().unwrap(),
                    prepared.simulation.as_ref().unwrap(),
                    prepared.path_id,
                    frame.time,
                )?
            };
            presentation.cam_tool_hidden = !state.paths;
            presentation.cam_tool = tool;
            presentation.cam_path_progress = progress;
            if applied.playback_stock_revision != Some(frame.stock_revision) {
                native_viewport::apply_interface_cam_stock(world, &session, frame.stock.clone())?;
                applied.stock_revision = native_viewport::interface_cam_stock_snapshot(world).0;
                applied.playback_stock_revision = Some(frame.stock_revision);
            }
            native_viewport::apply_interface_view(
                world,
                &session,
                None,
                Some(presentation.clone()),
            )?;
            applied.after_presentation = presentation;
            Ok(())
        })
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    (width, height, side): (f32, f32, f32),
    active: bool,
) -> Result<(), String> {
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    state.widgets.begin();
    let result: Result<(), String> = (|| {
        if !active {
            if let Some(pending) = &state.pending {
                pending.cancellation.cancel();
            }
            if state.pending.as_ref().is_some_and(|pending| {
                !matches!(
                    pending.receiver.lock().unwrap().try_recv(),
                    Err(mpsc::TryRecvError::Empty)
                )
            }) {
                state.pending = None;
            }
            restore(world, services, &mut state)?;
            state.key = None;
            state.document = None;
            state.setup = None;
            state.operation = None;
            state.warning = None;
            state.report = false;
            state.settings_open = false;
            state.prepared = None;
            state.nc_input = None;
            state.player = None;
            state.playback_action = None;
            state.request_pending = false;
            state.simulation_requested = false;
            state.seek_target = None;
            return Ok(());
        }
        let receipt = services
            .bridge
            .native_document_receipt(&services.engine, owner)?;
        let key = Key {
            owner: owner.clone(),
            revision: receipt.revision,
            selection: cam::selected(world),
        };
        if state.key.as_ref() != Some(&key) {
            restore(world, services, &mut state)?;
            if let Some(pending) = &state.pending {
                pending.cancellation.cancel();
            }
            let (document, setup, operation, warning) = services
                .bridge
                .with_native_document_receipt(&services.engine, owner, |revision| {
                    if revision != key.revision {
                        return Err("CAM document changed while preparing preview".into());
                    }
                    let document = services.engine.cam_document_snapshot();
                    let (setup, operation) = selection(&document, key.selection);
                    let warning = setup
                        .map(|id| {
                            services
                                .engine
                                .cam_snapshot(id)
                                .map(|(_, _, warning)| warning)
                        })
                        .transpose()?
                        .flatten();
                    Ok((document, setup, operation, warning))
                })?;
            state.warning = warning;
            if state
                .key
                .as_ref()
                .is_none_or(|prior| prior.owner != key.owner)
            {
                state.view = View::Stock;
                state.paths = true;
                state.settings = settings::Settings::default();
            }
            state.key = Some(key.clone());
            state.report = false;
            state.settings_open = false;
            state.document = Some(Arc::new(document));
            state.setup = setup;
            state.operation = operation;
            state.prepared = None;
            state.player = None;
            state.playback_action = None;
            state.error.clear();
            state.simulation_requested = false;
            state.nc_input = None;
            state.request_pending = setup.is_some();
            state.dirty = true;
            state.generation = state.generation.wrapping_add(1);
        }
        let completed = state.pending.as_ref().and_then(|pending| {
            match pending.receiver.lock().unwrap().try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("CAM preview worker stopped".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            }
        });
        if let Some(result) = completed {
            let pending = state.pending.take().unwrap();
            if state.key.as_ref() == Some(&pending.key) && state.generation == pending.generation {
                match result {
                    Ok(prepared) => state.prepared = Some(prepared),
                    Err(error) => state.error = error,
                }
                state.dirty = true;
            }
        }
        if state.request_pending && state.pending.is_none() {
            state.request_pending = false;
            let document = state.document.as_ref().unwrap().clone();
            let setup_id = state.setup.ok_or("Choose a CAM setup")?;
            let operation = state.operation;
            let request = state
                .simulation_requested
                .then(|| {
                    simulation_request(
                        world,
                        &document,
                        document.setup(setup_id).unwrap(),
                        operation,
                        state.settings,
                    )
                })
                .transpose()?;
            let cancellation = CamSimulationCancellation::default();
            let cancel = cancellation.clone();
            let warning = state.warning.clone();
            let nc_input = state.nc_input.clone();
            let (send, receiver) = mpsc::channel();
            let wake = world.get_resource::<NativeInterfaceHandle>().cloned();
            std::thread::Builder::new()
                .name("cad-native-cam-view".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if let Some(input) = nc_input {
                            nc_prepare::prepare(
                                &document,
                                &input,
                                request.ok_or("NC simulation request is missing")?,
                                &cancel,
                                warning,
                            )
                        } else {
                            prepare(&document, setup_id, operation, request, &cancel, warning)
                        }
                    }))
                    .unwrap_or_else(|_| Err("CAM preview worker stopped unexpectedly".into()));
                    let _ = send.send(result);
                    if let Some(handle) = wake {
                        handle.request_redraw();
                    }
                })
                .map_err(|e| e.to_string())?;
            state.pending = Some(Pending {
                key,
                generation: state.generation,
                cancellation,
                receiver: Mutex::new(receiver),
            });
            state.request_pending = false;
        }
        let picking_active = cam::geometry_pick::active(world);
        let frame_changed = advance_playback(world, &mut state)?;
        if frame_changed && picking_active {
            state.dirty = true;
        }
        if state.dirty {
            display(world, services, &mut state)?;
            state.dirty = false;
        } else if frame_changed {
            if let Err(error) = display_frame(world, services, &mut state) {
                state.player = None;
                return Err(error);
            }
        }
        let available = state.setup.is_some();
        let pending = state.pending.is_some();
        let x = (side + 12.).max(270.);
        let available_width = (width - x - 156.).max(100.);
        let columns = ((available_width / 124.).floor() as usize).clamp(1, 6);
        let cell_width = (available_width / columns as f32).min(124.);
        let simulated = state
            .prepared
            .as_ref()
            .and_then(|p| p.simulation.as_ref())
            .is_some();
        let playing = state.player.as_ref().is_some_and(|p| p.playing);
        let edit_dirty = cam::editing_dirty(world) || picking_active;
        let controls = [
            (
                "Model",
                Command::View(View::Model),
                state.view == View::Model,
                !available,
            ),
            (
                "Stock",
                Command::View(View::Stock),
                state.view == View::Stock,
                !available,
            ),
            (
                "Compare",
                Command::View(View::Compare),
                state.view == View::Compare,
                !available,
            ),
            ("Show toolpaths", Command::Paths, state.paths, !available),
            (
                "Simulate",
                Command::Simulate,
                false,
                !available || pending || cam::editing_dirty(world),
            ),
            (
                "Cancel simulation",
                Command::Cancel,
                false,
                !pending && state.player.is_none(),
            ),
            (
                "NC simulation",
                Command::Nc(0, nc_dialog::Command::Open),
                false,
                !available || edit_dirty,
            ),
            ("Start", Command::Start, false, !simulated || edit_dirty),
            (
                "Previous move",
                Command::Back,
                false,
                !simulated || edit_dirty,
            ),
            (
                if playing { "Pause" } else { "Play" },
                Command::Play,
                playing,
                !simulated || edit_dirty,
            ),
            (
                "Next move",
                Command::Forward,
                false,
                !simulated || edit_dirty,
            ),
            ("End", Command::End, false, !simulated || edit_dirty),
            (
                match state.player.as_ref().map_or(1., |p| p.speed) {
                    0.25 => "Speed 0.25x",
                    0.5 => "Speed 0.5x",
                    2. => "Speed 2x",
                    5. => "Speed 5x",
                    10. => "Speed 10x",
                    _ => "Speed 1x",
                },
                Command::Speed,
                false,
                !simulated,
            ),
        ];
        let count = if simulated { controls.len() } else { 7 };
        {
            let fill = crate::native_viewport::ui::theme(world)
                .panel
                .with_alpha(0.98);
            super::card(
                (&mut state.widgets, world, camera),
                "cam-view-toolbar",
                rect(
                    x - 8.,
                    122.,
                    cell_width * columns as f32 + 12.,
                    count.div_ceil(columns) as f32 * 32. + if simulated { 108. } else { 82. },
                ),
                fill,
                6.,
                44,
            )
        };
        for (i, (label, command, selected, disabled)) in
            controls.into_iter().take(count).enumerate()
        {
            let mut control = InterfaceControl::button("cam/view", label);
            control.disabled = disabled || picking_active;
            control.selected = Some(selected);
            state.widgets.button(
                world,
                camera,
                &format!("cam-view-{i}"),
                control,
                Some(label),
                NativeCommand::Workbench(super::Command::CamView(command)),
                rect(
                    x + (i % columns) as f32 * cell_width,
                    128. + (i / columns) as f32 * 32.,
                    cell_width - 4.,
                    28.,
                ),
                None,
                45,
            )?;
        }
        let mut settings_control = InterfaceControl::button("cam/view", "Simulation settings");
        settings_control.disabled = state.setup.is_none() || edit_dirty;
        state.widgets.button(
            world,
            camera,
            "cam-view-settings",
            settings_control,
            Some("Settings"),
            NativeCommand::Workbench(super::Command::CamView(Command::Settings)),
            rect(x, 128. + count.div_ceil(columns) as f32 * 32., 84., 24.),
            None,
            45,
        )?;
        let mut report_control = InterfaceControl::button("cam/view", "Report");
        report_control.disabled =
            picking_active || (state.prepared.is_none() && state.error.is_empty());
        state.widgets.button(
            world,
            camera,
            "cam-view-report",
            report_control,
            Some("Report"),
            NativeCommand::Workbench(super::Command::CamView(Command::Report)),
            rect(
                x + (cell_width * columns as f32 - 78.).max(0.),
                128. + count.div_ceil(columns) as f32 * 32.,
                74.,
                24.,
            ),
            None,
            45,
        )?;
        let playback_caption = state.player.as_ref().map(|p| {
            format!(
                "{} {:.1} / {:.1} s · {}x{}",
                if p.busy && p.frame.is_none() {
                    "Preparing playback"
                } else if p.playing {
                    "Playing"
                } else {
                    "Paused"
                },
                p.time(),
                state
                    .prepared
                    .as_ref()
                    .and_then(|p| p.simulation.as_ref())
                    .map_or(0., |s| s.estimated_seconds),
                p.speed,
                state
                    .prepared
                    .as_ref()
                    .and_then(|p| p.simulation.as_ref())
                    .and_then(|simulation| timeline::step_at(simulation, p.time()))
                    .and_then(|step| step.source_line)
                    .map_or(String::new(), |line| format!(" · NC block {line}"))
            )
        });
        let status_y = 158. + count.div_ceil(columns) as f32 * 32.;
        if let Some(prepared) = state.prepared.as_ref().filter(|_| simulated) {
            let end = prepared.simulation.as_ref().unwrap().estimated_seconds;
            let mut control = InterfaceControl::button("cam/view", "Playback time");
            control.disabled = edit_dirty || end <= prepared.start_time;
            control.field = limo_cad_interface::Field::Range {
                value: state
                    .player
                    .as_ref()
                    .map_or(end, |p| p.time().max(prepared.start_time)),
                min: prepared.start_time,
                max: end.max(prepared.start_time + 1e-9),
                step: 0.01,
            };
            state.widgets.button(
                world,
                camera,
                "cam-playback-time",
                control,
                None,
                NativeCommand::Workbench(super::Command::CamView(Command::Seek)),
                rect(
                    x,
                    status_y,
                    (cell_width * columns as f32 - 4.).max(80.),
                    24.,
                ),
                None,
                45,
            )?;
        }
        let message = if !state.error.is_empty() {
            state.error.as_str()
        } else if let Some(caption) = &playback_caption {
            caption
        } else if pending {
            if state.simulation_requested {
                "Simulating applied CAM document…"
            } else {
                "Preparing CAM preview…"
            }
        } else {
            state
                .prepared
                .as_ref()
                .map_or("Choose a setup or toolpath", |p| p.message.as_str())
        };
        state.widgets.text(
            world,
            camera,
            "cam-view-status",
            rect(
                (side + 12.).max(270.),
                status_y + if simulated { 26. } else { 0. },
                (cell_width * columns as f32 - 4.).max(80.),
                48.,
            ),
            message,
            12.,
            45,
        );
        if let Some(entity) = state.widgets.entity("cam-view-status") {
            let ink = crate::native_viewport::ui::theme(world).ink;
            world
                .entity_mut(entity)
                .insert(TextColor(if state.error.is_empty() {
                    ink
                } else {
                    Color::srgb(0.95, 0.35, 0.3)
                }));
        }
        if state.report {
            report::paint(world, camera, &mut state, width, side)?;
        }
        if state.settings_open {
            settings::paint(world, camera, &mut state, width, side)?;
        }
        Ok(())
    })();
    if let Err(error) = &result {
        state.error = error.clone();
    }
    state.widgets.finish(world);
    world.insert_resource(state);
    let dialog = nc_dialog::synchronize(world, camera, width, height);
    result.and(dialog)
}
