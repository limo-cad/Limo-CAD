//! Readiness for the embedded viewport, using Bevy 0.19's render-world APIs.
//!
//! Like Bevy's loading-screen example, assets and pipelines are independent
//! requirements. Unlike a queue-length test, this also requires prepared GPU
//! assets, a nonempty successful pipeline cache, and a presented real-size
//! viewport frame for the current document. The shared result works with both
//! the synchronous and pipelined render sub-apps.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bevy::{
    asset::{
        AssetId, LoadState, RecursiveDependencyLoadState, UntypedAssetId, VisitAssetDependencies,
    },
    pbr::{PreparedMaterial, SpecializedMaterialPipelineCache},
    prelude::*,
    render::{
        camera::ExtractedCamera,
        erased_render_asset::ErasedRenderAssets,
        mesh::RenderMesh,
        render_asset::RenderAssets,
        render_resource::{CachedPipelineState, Pipeline, PipelineCache},
        renderer::render_system,
        sync_world::MainEntity,
        texture::GpuImage,
        view::{window::ExtractedWindows, ViewTarget},
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    },
    shader::ShaderCacheError,
    text::FontSource,
};

use super::{
    CamStockResource, CameraResource, HudResource, ModelResource, NativeModelGeometry,
    PresentationResource, PreviewResource, RenderedRevisions, ViewportSizeResource,
};

#[derive(Clone, Default)]
pub(super) struct Report {
    pub ready: bool,
    pub phase: &'static str,
    pub error: Option<String>,
    pub session_id: String,
    pub geometry_revision: u64,
    model_revision: u64,
    pub pipeline_count: usize,
    pub uploaded_mesh_count: usize,
    has_been_ready: bool,
}

#[derive(Resource, Clone)]
struct SharedReport(Arc<Mutex<Report>>);

#[derive(Resource)]
struct LoadingState {
    started: Instant,
    real_layout: bool,
}

#[derive(Resource, Clone, Default)]
struct Requirements {
    session_id: String,
    geometry_revision: u64,
    model_revision: u64,
    real_layout: bool,
    cpu_ready: bool,
    error: Option<String>,
    meshes: Vec<AssetId<Mesh>>,
    materials: Vec<AssetId<StandardMaterial>>,
    images: Vec<AssetId<Image>>,
    visible_material_entities: Vec<Entity>,
}

#[derive(Resource, Default)]
struct FrameEvidence {
    acquired_window: Option<Entity>,
}

pub(super) fn install(app: &mut App) {
    let shared = SharedReport(Arc::new(Mutex::new(Report {
        phase: "assets",
        ..default()
    })));
    app.insert_resource(shared.clone())
        .insert_resource(LoadingState {
            started: Instant::now(),
            real_layout: false,
        })
        .init_resource::<Requirements>()
        .add_systems(Last, collect_requirements);
    app.sub_app_mut(RenderApp)
        .insert_resource(shared)
        .init_resource::<Requirements>()
        .init_resource::<FrameEvidence>()
        .add_systems(ExtractSchedule, extract_requirements)
        .add_systems(
            Render,
            (
                capture_acquired_frame.before(render_system),
                publish_report.after(render_system),
            )
                .in_set(RenderSystems::Render),
        );
}

pub(super) fn layout_received(app: &mut App) {
    if let Some(mut state) = app.world_mut().get_resource_mut::<LoadingState>() {
        state.real_layout = true;
    }
}

pub(super) fn initial_loading(app: &App) -> bool {
    app.world()
        .get_resource::<SharedReport>()
        .is_some_and(|shared| shared.0.lock().is_ok_and(|report| !report.has_been_ready))
}

pub(super) fn report(app: &App) -> Option<Report> {
    let shared = app.world().get_resource::<SharedReport>()?;
    let mut report = shared.0.lock().ok()?.clone();
    let model = app.world().get_resource::<ModelResource>()?;
    // A pipelined render may still be publishing the preceding document.
    // Never identify that completed frame as the newly requested document.
    if report.session_id != model.session_id
        || report.geometry_revision != model.geometry_revision
        || report.model_revision != model.revision
    {
        report.ready = false;
        report.phase = "gpu";
    }
    let state = app.world().get_resource::<LoadingState>()?;
    if !report.ready
        && !report.has_been_ready
        && report.error.is_none()
        && state.started.elapsed() > Duration::from_secs(60)
    {
        let pending_phase = report.phase;
        report.phase = "error";
        report.error = Some(format!("Viewport preparation timed out during {pending_phase}. Retry startup or open an empty document."));
    }
    Some(report)
}

fn collect_requirements(
    mut required: ResMut<Requirements>,
    state: Res<LoadingState>,
    shared: Res<SharedReport>,
    model: Res<ModelResource>,
    (hud, preview, stock, size, camera, presentation): (
        Res<HudResource>,
        Res<PreviewResource>,
        Res<CamStockResource>,
        Res<ViewportSizeResource>,
        Res<CameraResource>,
        Res<PresentationResource>,
    ),
    revisions: Res<RenderedRevisions>,
    asset_server: Res<AssetServer>,
    (meshes, materials, images, fonts, shaders): (
        Res<Assets<Mesh>>,
        Res<Assets<StandardMaterial>>,
        Res<Assets<Image>>,
        Res<Assets<Font>>,
        Res<Assets<Shader>>,
    ),
    mesh_entities: Query<(
        Entity,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
        Option<&NativeModelGeometry>,
        Option<&Visibility>,
        Option<&ViewVisibility>,
    )>,
    text_fonts: Query<&TextFont>,
    mut previous: Local<Option<(String, u64, u64, u64, u64, u64, u32, u32)>>,
) {
    let key = (
        model.session_id.clone(),
        model.geometry_revision,
        model.revision,
        hud.revision,
        preview.revision,
        stock.revision,
        size.logical_width.to_bits(),
        size.logical_height.to_bits(),
    );
    if previous.as_ref() == Some(&key)
        && !presentation.is_changed()
        && shared.0.lock().is_ok_and(|report| report.ready)
    {
        if camera.is_changed() {
            // Camera motion changes visibility without changing any assets.
            // Refresh only draw requirements; retain the prepared-asset scan.
            required.visible_material_entities = mesh_entities
                .iter()
                .filter(|(_, _, _, geometry, visibility, view_visibility)| {
                    !visibility.is_some_and(|visibility| *visibility == Visibility::Hidden)
                        && !geometry.is_some_and(|geometry| geometry.session_id != model.session_id)
                        && view_visibility.is_some_and(|visibility| visibility.get())
                })
                .map(|(entity, ..)| entity)
                .collect();
        }
        return;
    }
    *previous = Some(key);
    let mut next = Requirements {
        session_id: model.session_id.clone(),
        geometry_revision: model.geometry_revision,
        model_revision: model.revision,
        real_layout: state.real_layout,
        cpu_ready: revisions.model == model.revision,
        ..default()
    };
    let mut assets = Vec::<UntypedAssetId>::new();
    for (entity, mesh, material, geometry, visibility, view_visibility) in &mesh_entities {
        if visibility.is_some_and(|v| *v == Visibility::Hidden)
            || geometry.is_some_and(|g| g.session_id != model.session_id)
        {
            continue;
        }
        next.meshes.push(mesh.0.id());
        next.materials.push(material.0.id());
        if view_visibility.is_some_and(|visibility| visibility.get()) {
            next.visible_material_entities.push(entity);
        }
        next.cpu_ready &= meshes.contains(&mesh.0) && materials.contains(&material.0);
        assets.push(mesh.0.id().untyped());
        assets.push(material.0.id().untyped());
        if let Some(value) = materials.get(&material.0) {
            value.visit_dependencies(&mut |id| assets.push(id));
        }
    }
    for font in &text_fonts {
        if let FontSource::Handle(handle) = &font.font {
            next.cpu_ready &= fonts.contains(handle);
            assets.push(handle.id().untyped());
        }
    }
    // This viewport's image assets include text atlases and its embedded
    // tonemapping LUTs. Waiting for GPU preparation avoids a blank first HUD.
    next.images.extend(images.ids());
    assets.extend(next.images.iter().map(|id| id.untyped()));
    assets.extend(shaders.ids().map(|id| id.untyped()));
    for id in assets {
        match asset_server.get_load_state(id) {
            Some(LoadState::Failed(error)) => {
                next.error = Some(format!("Required viewport asset failed: {error}"))
            }
            Some(LoadState::Loaded) => match asset_server.get_recursive_dependency_load_state(id) {
                Some(RecursiveDependencyLoadState::Failed(error)) => {
                    next.error = Some(format!(
                        "Required viewport asset dependency failed: {error}"
                    ))
                }
                Some(RecursiveDependencyLoadState::Loaded) => {}
                _ => next.cpu_ready = false,
            },
            Some(_) => next.cpu_ready = false,
            // Procedural meshes, materials, fonts and embedded shaders are
            // inserted directly into Assets; AssetServer does not own them.
            None => {}
        }
    }
    *required = next;
}

fn extract_requirements(mut commands: Commands, required: Extract<Res<Requirements>>) {
    commands.insert_resource((**required).clone());
}

fn capture_acquired_frame(
    mut evidence: ResMut<FrameEvidence>,
    required: Res<Requirements>,
    windows: Res<ExtractedWindows>,
    cameras: Query<(&ExtractedCamera, &ViewTarget)>,
) {
    evidence.acquired_window = None;
    if !required.real_layout {
        return;
    }
    for window in windows.values() {
        if window.physical_width <= 1
            || window.physical_height <= 1
            || window.swap_chain_texture.is_none()
        {
            continue;
        }
        if cameras.iter().any(|(camera, _)| matches!(camera.target,
            Some(bevy::camera::NormalizedRenderTarget::Window(target)) if target.entity() == window.entity)) {
            evidence.acquired_window = Some(window.entity);
            break;
        }
    }
}

fn publish_report(
    shared: Res<SharedReport>,
    required: Res<Requirements>,
    evidence: Res<FrameEvidence>,
    windows: Res<ExtractedWindows>,
    cache: Res<PipelineCache>,
    specialized_materials: Res<SpecializedMaterialPipelineCache>,
    meshes: Res<RenderAssets<RenderMesh>>,
    materials: Res<ErasedRenderAssets<PreparedMaterial>>,
    images: Res<RenderAssets<GpuImage>>,
) {
    let mut next = Report {
        phase: "assets",
        session_id: required.session_id.clone(),
        geometry_revision: required.geometry_revision,
        model_revision: required.model_revision,
        error: required.error.clone(),
        ..default()
    };
    next.uploaded_mesh_count = required
        .meshes
        .iter()
        .filter(|id| meshes.get(**id).is_some())
        .count();
    let gpu_ready = next.uploaded_mesh_count == required.meshes.len()
        && required
            .materials
            .iter()
            .all(|id| materials.get(*id).is_some())
        && required.images.iter().all(|id| images.get(*id).is_some());
    let mut pipelines_ready = true;
    for pipeline in cache.pipelines() {
        match &pipeline.state {
            CachedPipelineState::Ok(Pipeline::RenderPipeline(_)) => next.pipeline_count += 1,
            CachedPipelineState::Ok(Pipeline::ComputePipeline(_)) => {}
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => pipelines_ready = false,
            CachedPipelineState::Err(error) => {
                next.error = Some(format!("Viewport shader pipeline failed: {error}"));
                pipelines_ready = false;
            }
            _ => pipelines_ready = false,
        }
    }
    pipelines_ready = pipeline_evidence_ready(
        next.pipeline_count,
        pipelines_ready,
        cache.waiting_pipelines().next().is_none(),
    );
    // Uploaded assets alone do not prove their material pipelines were
    // specialized: Bevy can defer a mesh while its instance/view is prepared.
    // Require actual successful pipeline IDs for the meshes visible this frame.
    // Offscreen geometry does not need a draw pipeline until it becomes visible.
    pipelines_ready &= required.visible_material_entities.iter().all(|entity| {
        let entity = MainEntity::from(*entity);
        specialized_materials.values().any(|view| {
            view.get(&entity)
                .is_some_and(|pipeline| cache.get_render_pipeline(*pipeline).is_some())
        })
    });
    let presented = evidence
        .acquired_window
        .and_then(|id| windows.get(&id))
        .is_some_and(|window| window.swap_chain_texture.is_none() && !window.needs_initial_present);
    next.phase = readiness_phase(required.cpu_ready, gpu_ready, pipelines_ready, presented);
    next.ready = next.phase == "ready" && next.error.is_none();
    if next.error.is_some() {
        next.phase = "error";
    }
    if let Ok(mut report) = shared.0.lock() {
        next.has_been_ready = report.has_been_ready || next.ready;
        *report = next;
    }
}

fn readiness_phase(assets: bool, gpu: bool, pipelines: bool, presented: bool) -> &'static str {
    if !assets {
        "assets"
    } else if !gpu {
        "gpu"
    } else if !pipelines {
        "pipelines"
    } else if !presented {
        "frame"
    } else {
        "ready"
    }
}

fn pipeline_evidence_ready(
    successful_render_count: usize,
    all_successful: bool,
    queue_empty: bool,
) -> bool {
    successful_render_count > 0 && all_successful && queue_empty
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_requires_each_independent_stage() {
        assert_eq!(readiness_phase(false, true, true, true), "assets");
        assert_eq!(readiness_phase(true, false, true, true), "gpu");
        assert_eq!(readiness_phase(true, true, false, true), "pipelines");
        assert_eq!(readiness_phase(true, true, true, false), "frame");
        assert_eq!(readiness_phase(true, true, true, true), "ready");
    }
    #[test]
    fn an_empty_pipeline_cache_is_not_readiness_evidence() {
        assert!(!pipeline_evidence_ready(0, true, true));
        assert!(!pipeline_evidence_ready(4, false, true));
        assert!(!pipeline_evidence_ready(4, true, false));
        assert!(pipeline_evidence_ready(4, true, true));
    }

    fn test_app(published: Report, elapsed: Duration) -> App {
        let mut app = App::new();
        app.insert_resource(SharedReport(Arc::new(Mutex::new(published))))
            .insert_resource(LoadingState {
                started: Instant::now() - elapsed,
                real_layout: true,
            })
            .insert_resource(ModelResource {
                session_id: "current".to_string(),
                geometry_revision: 7,
                ..default()
            });
        app
    }

    #[test]
    fn stale_document_frame_never_releases_the_overlay() {
        for (session_id, geometry_revision) in [("previous", 7), ("current", 6)] {
            let app = test_app(
                Report {
                    ready: true,
                    phase: "ready",
                    session_id: session_id.to_string(),
                    geometry_revision,
                    ..default()
                },
                Duration::ZERO,
            );
            assert!(!report(&app).unwrap().ready);
        }
        let app = test_app(
            Report {
                ready: true,
                phase: "ready",
                session_id: "current".to_string(),
                geometry_revision: 7,
                ..default()
            },
            Duration::ZERO,
        );
        assert!(report(&app).unwrap().ready);
    }

    #[test]
    fn timeout_applies_to_startup_and_retains_the_stalled_phase() {
        let app = test_app(
            Report {
                phase: "pipelines",
                session_id: "current".to_string(),
                geometry_revision: 7,
                ..default()
            },
            Duration::from_secs(61),
        );
        assert!(report(&app)
            .unwrap()
            .error
            .unwrap()
            .contains("during pipelines"));
        let app = test_app(
            Report {
                phase: "pipelines",
                session_id: "current".to_string(),
                geometry_revision: 7,
                has_been_ready: true,
                ..default()
            },
            Duration::from_secs(61),
        );
        assert!(report(&app).unwrap().error.is_none());
    }

    #[test]
    fn pose_or_appearance_update_requires_a_new_document_frame() {
        let mut app = test_app(
            Report {
                ready: true,
                phase: "ready",
                session_id: "current".to_string(),
                geometry_revision: 7,
                ..default()
            },
            Duration::ZERO,
        );
        assert!(report(&app).unwrap().ready);
        app.world_mut().resource_mut::<ModelResource>().revision += 1;
        assert!(!report(&app).unwrap().ready);
    }
}
