//! Isolated teaching geometry in the existing CAD renderer. No live-scene resources are changed.
use super::*;
use crate::native_viewport::script_preview::{
    PreviewCue, PreviewDocument, PreviewFinish, RenderRequest,
};

#[path = "teaching_animation.rs"]
mod animation;
#[path = "teaching_inspection.rs"]
mod inspection;
const LAYER: usize = 2;
#[derive(Component)]
struct TeachingOwned;
#[derive(Component)]
pub(super) struct TeachingCamera;
#[derive(Default, Reflect, GizmoConfigGroup)]
struct TeachingEdges;
#[derive(Default, Reflect, GizmoConfigGroup)]
struct TeachingSelection;

#[derive(Resource, Default)]
struct Scene {
    active: bool,
    view_closed: Option<Arc<AtomicBool>>,
    animation: animation::Motion,
    progress: f32,
    geometry_frame: Option<usize>,
    transform: Transform,
    key: Option<(String, usize)>,
    palette: Option<ViewportPalette>,
    geometry: Vec<Entity>,
    fixtures: Vec<Entity>,
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
    solid: Option<Arc<SolidSceneDto>>,
    profile: Option<limo_cad_sketch::SketchDto>,
    plane: Option<([f64; 2], [f64; 2])>,
    selected: Vec<u64>,
    inspection: Option<crate::native_viewport::script_preview::inspection::Inspection>,
    datum_bounds: ([f64; 2], [f64; 2]),
    camera: Option<ViewportCamera>,
    viewport: Option<ViewportSizeResource>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Scene>()
        .init_gizmo_group::<TeachingEdges>()
        .init_gizmo_group::<TeachingSelection>()
        .add_systems(Update, draw);
    let mut configs = app.world_mut().resource_mut::<GizmoConfigStore>();
    let edges = configs.config_mut::<TeachingEdges>().0;
    edges.render_layers = RenderLayers::layer(LAYER);
    edges.line.width = 1.5;
    edges.depth_bias = 0.0;
    let selected = configs.config_mut::<TeachingSelection>().0;
    selected.render_layers = RenderLayers::layer(LAYER);
    selected.line.width = 2.0;
    selected.depth_bias = DIRECT_PICK_FEEDBACK_DEPTH_BIAS;
}

pub(super) fn show(
    world: &mut World,
    document: &PreviewDocument,
    request: &RenderRequest,
    target: Handle<Image>,
    progress: f32,
) -> Result<(), String> {
    let frame = document
        .frames
        .get(request.frame_index)
        .ok_or("Unknown teaching frame")?;
    if !(64..=640).contains(&request.width) || !(64..=400).contains(&request.height) {
        return Err("Teaching target exceeds its size budget".into());
    }
    let camera = document.camera(request);
    let palette = request
        .palette
        .unwrap_or(world.resource::<PaletteResource>().0);
    let key = (request.preview_id.clone(), request.frame_index);
    let changed = world.resource::<Scene>().key.as_ref() != Some(&key);
    if changed {
        world.resource_mut::<Scene>().animation =
            animation::Motion::new(document, request.frame_index);
    }
    let source_index = world
        .resource::<Scene>()
        .animation
        .source(request.frame_index, progress);
    let source = &document.frames[source_index];
    let rebuild = changed
        || world.resource::<Scene>().geometry_frame != Some(source_index)
        || world.resource::<Scene>().palette != Some(palette);
    if rebuild {
        clear_geometry(world);
        let mut entities = Vec::new();
        let mut mesh_handles = Vec::new();
        let mut material_handles = Vec::new();
        for body in &source.scene.bodies {
            for (mesh, accent) in inspection::meshes(body, source.inspection.as_ref()) {
                let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
                let material =
                    world
                        .resource_mut::<Assets<StandardMaterial>>()
                        .add(StandardMaterial {
                            base_color: if accent {
                                rgb(palette.accent)
                            } else if matches!(frame.finish, Some(PreviewFinish::SatinAluminum)) {
                                Color::srgb(0.75, 0.78, 0.79)
                            } else {
                                rgb(palette.body)
                            },
                            metallic: if frame.finish.is_some() { 0.55 } else { 0.0 },
                            perceptual_roughness: if frame.finish.is_some() { 0.34 } else { 0.86 },
                            cull_mode: None,
                            ..default()
                        });
                entities.push(
                    world
                        .spawn((
                            TeachingOwned,
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::default(),
                            RenderLayers::layer(LAYER),
                        ))
                        .id(),
                );
                mesh_handles.push(mesh);
                material_handles.push(material);
            }
        }
        let mut scene = world.resource_mut::<Scene>();
        scene.geometry = entities;
        scene.meshes = mesh_handles;
        scene.materials = material_handles;
        scene.solid = Some(source.scene.clone());
        scene.geometry_frame = Some(source_index);
        scene.profile = frame.profile.clone().filter(|p| {
            frame
                .inspection
                .as_ref()
                .is_none_or(|i| i.profile_visible(&p.name))
        });
        scene.inspection = frame.inspection.clone();
        scene.datum_bounds = plane_bounds(document);
        scene.plane = (frame.cue == Some(PreviewCue::Plane)).then(|| plane_bounds(document));
        scene.selected = source.edges.clone();
        scene.key = Some(key);
        scene.palette = Some(palette);
    }
    let transform = world.resource::<Scene>().animation.transform(progress);
    let entities = world.resource::<Scene>().geometry.clone();
    for entity in entities {
        if let Some(mut value) = world.get_mut::<Transform>(entity) {
            *value = transform;
        }
    }
    {
        let mut scene = world.resource_mut::<Scene>();
        scene.progress = progress;
        scene.transform = transform;
    }
    // Retain camera/light entity identities across orbit, resize and reopen.
    let mut fixtures = world.resource::<Scene>().fixtures.clone();
    if fixtures.is_empty() {
        fixtures = (0..3).map(|_| world.spawn_empty().id()).collect();
    }
    let distance = Vec3::from_array(camera.position).distance(Vec3::from_array(camera.target));
    world.entity_mut(fixtures[0]).insert((
        TeachingOwned,
        TeachingCamera,
        Camera3d::default(),
        bevy::light::AmbientLight {
            brightness: 350.0,
            ..default()
        },
        ClusterConfig::None,
        VIEWPORT_MSAA,
        Camera {
            clear_color: ClearColorConfig::Custom(rgb(palette.background)),
            ..default()
        },
        bevy::camera::RenderTarget::Image(target.into()),
        Projection::Perspective(PerspectiveProjection {
            fov: camera.vertical_fov_degrees.to_radians(),
            near: (distance / 100_000.0).max(0.001),
            far: (distance * 3.0).max(100.0),
            ..default()
        }),
        camera_transform(camera),
        RenderLayers::layer(LAYER),
    ));
    let (key_light, fill_light) = camera_relative_light_transforms(camera);
    for (entity, (transform, illuminance)) in fixtures[1..]
        .iter()
        .zip([(key_light, 3_200.0), (fill_light, 650.0)])
    {
        world.entity_mut(*entity).insert((
            TeachingOwned,
            DirectionalLight {
                illuminance,
                shadow_maps_enabled: false,
                ..default()
            },
            transform,
            RenderLayers::layer(LAYER),
        ));
    }
    let mut scene = world.resource_mut::<Scene>();
    scene.fixtures = fixtures;
    scene.camera = Some(camera);
    scene.viewport = Some(ViewportSizeResource {
        logical_width: request.width as f32,
        logical_height: request.height as f32,
    });
    scene.active = true;
    Ok(())
}

pub(super) fn hide(world: &mut World) {
    world.resource_mut::<Scene>().active = false;
    for mut camera in world
        .query_filtered::<&mut Camera, With<TeachingCamera>>()
        .iter_mut(world)
    {
        camera.is_active = false;
    }
}

pub(super) fn bind_view(world: &mut World, closed: Arc<AtomicBool>) {
    world.resource_mut::<Scene>().view_closed = Some(closed);
}

/// A delayed Close cannot clear a newer view's geometry or fixtures.
pub(super) fn release_closed(world: &mut World) -> bool {
    if world
        .resource::<Scene>()
        .view_closed
        .as_ref()
        .is_some_and(|closed| closed.load(Ordering::Acquire))
    {
        release(world);
        true
    } else {
        false
    }
}

#[cfg(test)]
pub(super) fn resident_scene(world: &World) -> Option<std::sync::Weak<SolidSceneDto>> {
    world.resource::<Scene>().solid.as_ref().map(Arc::downgrade)
}

fn clear_geometry(world: &mut World) {
    let (entities, meshes, materials) = {
        let mut scene = world.resource_mut::<Scene>();
        (
            std::mem::take(&mut scene.geometry),
            std::mem::take(&mut scene.meshes),
            std::mem::take(&mut scene.materials),
        )
    };
    for entity in entities {
        world.despawn(entity);
    }
    for mesh in meshes {
        world.resource_mut::<Assets<Mesh>>().remove(mesh.id());
    }
    for material in materials {
        world
            .resource_mut::<Assets<StandardMaterial>>()
            .remove(material.id());
    }
}

pub(super) fn release(world: &mut World) {
    clear_geometry(world);
    let fixtures = std::mem::take(&mut world.resource_mut::<Scene>().fixtures);
    for entity in fixtures {
        world.despawn(entity);
    }
    *world.resource_mut::<Scene>() = Scene::default();
}

fn draw(
    scene: Res<Scene>,
    mut edges: Gizmos<TeachingEdges>,
    mut selected: Gizmos<TeachingSelection>,
) {
    keep_gizmo_asset_resident(&mut edges);
    keep_gizmo_asset_resident(&mut selected);
    if !scene.active {
        return;
    }
    let (Some(solid), Some(camera), Some(viewport), Some(palette)) =
        (&scene.solid, scene.camera, scene.viewport, scene.palette)
    else {
        return;
    };
    let transform = scene.transform;
    for body in &solid.bodies {
        if scene
            .inspection
            .as_ref()
            .and_then(|i| i.visibility.as_ref())
            .is_some_and(|v| v.hidden_body_ids.contains(&body.id.0))
        {
            continue;
        }
        if let Some(section) = scene
            .inspection
            .as_ref()
            .and_then(|i| i.section.as_ref())
            .filter(|s| s.request.body_id == body.id)
        {
            if let Some(cutaway) = &section.cutaway {
                for edge in &cutaway.edges {
                    for pair in edge.points.windows(2) {
                        let point = |p: &limo_cad_solid::Point3Dto| {
                            Vec3::new(p.x as f32, p.y as f32, p.z as f32)
                        };
                        edges.line(point(&pair[0]), point(&pair[1]), rgba(palette.edge, 0.92));
                    }
                }
            }
            continue;
        }
        let sides = edge_side_faces(body, &transform);
        for edge in &body.edges {
            let side = sides.get(edge.key.as_str()).copied().unwrap_or_default();
            draw_edge_segments(
                &mut edges,
                edge,
                rgba(palette.edge, 0.92),
                &transform,
                Some(EdgeLift {
                    camera,
                    viewport,
                    sides: side,
                    ceiling: MODEL_EDGE_MAX_LIFT_MM,
                }),
            );
            if inspection::edge_selected(scene.inspection.as_ref(), body, edge)
                || scene.selected.contains(&edge.id.0)
                    && scene
                        .animation
                        .edge_visible(edge.id.0, &scene.selected, scene.progress)
            {
                draw_edge_segments(
                    &mut selected,
                    edge,
                    rgb(palette.edge_selected),
                    &transform,
                    None,
                );
            }
        }
    }
    if let Some(i) = &scene.inspection {
        for datum in &i.datums {
            if i.visibility
                .as_ref()
                .is_some_and(|v| v.hidden_datum_plane_ids.contains(&datum.datum_id.0))
            {
                continue;
            }
            let (low, high) = scene.datum_bounds;
            for (a, b) in plane_segments(low, high) {
                let point = |p| {
                    let p = datum.basis.to_3d(p);
                    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
                };
                selected.line(point(a), point(b), rgba(palette.accent, 0.6));
            }
        }
    }
    if let Some(profile) = &scene.profile {
        // Keep the same continuous, depth-faded sketch grid as the document
        // viewport throughout drawing and dimensioning, not just Plane.
        draw_grid_on_basis(
            &mut edges,
            &profile.basis,
            grid_layout(camera, viewport, &profile.basis),
            camera,
            viewport,
            rgba(palette.grid_fine, 0.28),
            rgba(palette.grid_major, 0.48),
        );
        if let Some((low, high)) = scene.plane {
            for (index, (start, end)) in plane_segments(low, high).into_iter().enumerate() {
                let point = |p| {
                    let p = profile.basis.to_3d(p);
                    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
                };
                selected.line(
                    point(start),
                    point(start).lerp(point(end), scene.progress),
                    rgba(palette.accent, if index < 4 { 0.85 } else { 0.46 }),
                );
            }
        }
        let mut new_index = 0;
        let new_count = profile
            .entities
            .iter()
            .filter(|entity| scene.animation.is_new_curve(entity))
            .count();
        for entity in &profile.entities {
            let fraction = if scene.animation.is_new_curve(entity) {
                let fraction = (scene.progress * new_count as f32 - new_index as f32).clamp(0., 1.);
                new_index += 1;
                fraction
            } else {
                1.
            };
            let Some(entity) = animation::partial_curve(entity, fraction) else {
                continue;
            };
            draw_sketch_curve_at_offset(
                &mut selected,
                &profile.basis,
                &entity,
                if scene
                    .inspection
                    .as_ref()
                    .and_then(|i| i.selection.as_ref())
                    .is_none_or(|s| s.sketch_entity_ids.contains(&entity.id().0))
                {
                    rgb(palette.accent)
                } else {
                    rgba(palette.edge, 0.8)
                },
                DIRECT_PICK_FEEDBACK_OFFSET,
            );
        }
    }
    scene
        .animation
        .draw_guide(&mut selected, scene.progress, rgb(palette.accent));
}

// Match the forthcoming sketch's footprint while retaining the real local origin.
// The cue uses six bounded lines, not the main viewport's adaptive grid.
fn plane_bounds(document: &PreviewDocument) -> ([f64; 2], [f64; 2]) {
    let mut low = [0_f64; 2];
    let mut high = [0_f64; 2];
    for profile in document
        .frames
        .iter()
        .filter_map(|frame| frame.profile.as_ref())
    {
        for entity in &profile.entities {
            if let limo_cad_sketch::EntityDto::Line { start, end, .. } = entity {
                for point in [start, end] {
                    for (axis, value) in [point.x, point.y].into_iter().enumerate() {
                        low[axis] = low[axis].min(value);
                        high[axis] = high[axis].max(value);
                    }
                }
            }
        }
    }
    for axis in 0..2 {
        let margin = ((high[axis] - low[axis]) * 0.05).max(1.);
        low[axis] -= margin;
        high[axis] += margin;
    }
    (low, high)
}

fn plane_segments(low: [f64; 2], high: [f64; 2]) -> [([f64; 2], [f64; 2]); 6] {
    let corners = [low, [high[0], low[1]], high, [low[0], high[1]]];
    [
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[3]),
        (corners[3], corners[0]),
        ([low[0], 0.], [high[0], 0.]),
        ([0., low[1]], [0., high[1]]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn teaching_plane_cue_has_closed_boundary_and_true_origin_axes() {
        let lines = plane_segments([-2., -3.], [34., 47.]);
        for index in 0..4 {
            assert_eq!(lines[index].1, lines[(index + 1) % 4].0);
            assert_ne!(lines[index].0, lines[index].1);
        }
        assert_eq!(lines[4], ([-2., 0.], [34., 0.]));
        assert_eq!(lines[5], ([0., -3.], [0., 47.]));
    }
    #[test]
    fn teaching_scene_show_keeps_live_resources_and_reuses_camera_and_meshes() {
        let frame = serde_json::from_value(serde_json::json!({
            "caption":"sample", "scene":{"bodies":[{"id":1,"name":"Stock","feature_id":2,
            "faces":[],"edges":[],"mesh":{"positions":[0,0,0, 10,0,0, 0,10,0],
            "normals":[0,0,1,0,0,1,0,0,1],"indices":[0,1,2]}}],"errors":[]}
        }))
        .unwrap();
        let document = PreviewDocument::new(vec![frame]).unwrap();
        let mut world = World::new();
        world.init_resource::<Scene>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<PaletteResource>();
        world.init_resource::<ModelResource>();
        world.init_resource::<CameraResource>();
        world.init_resource::<PresentationResource>();
        let before = (
            world.resource::<ModelResource>().session_id.clone(),
            world.resource::<CameraResource>().revision,
            world.resource_ref::<PresentationResource>().last_changed(),
            world.resource_ref::<PaletteResource>().last_changed(),
        );
        let mut request = RenderRequest {
            preview_id: "teaching".into(),
            view_id: "view".into(),
            revision: 1,
            frame_index: 0,
            width: 300,
            height: 176,
            yaw: 0.5,
            pitch: 0.5,
            palette: None,
        };
        show(&mut world, &document, &request, Handle::default(), 1.0).unwrap();
        let fixtures = world.resource::<Scene>().fixtures.clone();
        let meshes = world.resource::<Scene>().meshes.clone();
        for progress in [0., 0.25, 0.5, 0.75, 1.] {
            show(&mut world, &document, &request, Handle::default(), progress).unwrap();
            assert_eq!(world.resource::<Scene>().meshes, meshes);
            assert_eq!(world.resource::<Scene>().fixtures, fixtures);
        }
        hide(&mut world);
        request.yaw = 0.8;
        show(&mut world, &document, &request, Handle::default(), 1.0).unwrap();
        assert_eq!(world.resource::<Scene>().fixtures, fixtures);
        assert_eq!(world.resource::<Scene>().meshes, meshes);
        assert_eq!(world.resource::<ModelResource>().session_id, before.0);
        assert_eq!(world.resource::<CameraResource>().revision, before.1);
        assert_eq!(
            world.resource_ref::<PresentationResource>().last_changed(),
            before.2
        );
        assert_eq!(
            world.resource_ref::<PaletteResource>().last_changed(),
            before.3
        );
        release(&mut world);
        assert!(world.resource::<Assets<Mesh>>().is_empty());
        assert!(world.resource::<Assets<StandardMaterial>>().is_empty());
    }

    #[test]
    fn teaching_scene_release_only_removes_owned_assets_and_entities() {
        let mut world = World::new();
        world.init_resource::<Scene>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        let live = world.spawn((Camera::default(), NativeViewportCamera)).id();
        let teaching = world
            .spawn((Camera::default(), TeachingCamera, TeachingOwned))
            .id();
        world.resource_mut::<Scene>().fixtures.push(teaching);
        hide(&mut world);
        assert!(world.get::<Camera>(live).unwrap().is_active);
        assert!(!world.get::<Camera>(teaching).unwrap().is_active);
        release(&mut world);
        assert!(world.get_entity(live).is_ok());
        assert!(world.get_entity(teaching).is_err());
        assert!(!RenderLayers::layer(LAYER).intersects(&RenderLayers::default()));
        assert!(!RenderLayers::layer(LAYER).intersects(&RenderLayers::layer(1)));
    }
}

#[cfg(test)]
mod close_resource_tests {
    use super::*;

    #[derive(Default, Resource)]
    struct CleanupExtractions(usize);

    #[test]
    fn native_teaching_close_flushes_without_cameras_and_restores_activation() {
        // Run the production retirement path and prewarm hook with a CPU-only
        // render sub-app. A cleanup pass must not need any GPU cache resources.
        let mut app = App::new();
        app.init_resource::<Scene>();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<StandardMaterial>>();
        let main = app
            .world_mut()
            .spawn((Camera::default(), NativeViewportCamera))
            .id();
        let overlay = app
            .world_mut()
            .spawn((Camera::default(), NativeViewportCamera))
            .id();
        let inactive = app
            .world_mut()
            .spawn(Camera {
                is_active: false,
                ..default()
            })
            .id();
        let teaching = app
            .world_mut()
            .spawn((Camera::default(), TeachingCamera, TeachingOwned))
            .id();
        app.world_mut().resource_mut::<Scene>().fixtures = vec![teaching];
        let solid = Arc::new(SolidSceneDto::default());
        let witness = Arc::downgrade(&solid);
        app.world_mut().resource_mut::<Scene>().solid = Some(solid);
        let closed = Arc::new(AtomicBool::new(false));
        bind_view(app.world_mut(), closed.clone());
        hide(app.world_mut());
        let mut render = bevy::app::SubApp::new();
        render.init_resource::<CleanupExtractions>();
        render.set_extract(|main, render| {
            assert!(main
                .query::<&Camera>()
                .iter(main)
                .all(|camera| !camera.is_active));
            assert_eq!(
                main.query_filtered::<Entity, With<TeachingCamera>>()
                    .iter(main)
                    .count(),
                0,
                "the teaching fixture must be removed before extraction"
            );
            super::super::surface_submission::prewarm_blit_pipelines(main, render);
            render.resource_mut::<CleanupExtractions>().0 += 1;
        });
        app.insert_sub_app(RenderApp, render);
        assert!(
            !super::super::retire_teaching(&mut app),
            "a hidden, still-open teaching scene must remain resident"
        );
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<CleanupExtractions>()
                .0,
            0
        );
        closed.store(true, Ordering::Release);
        assert!(super::super::retire_teaching(&mut app));
        assert!(witness.upgrade().is_none());
        assert!(app.world().get_entity(teaching).is_err());
        assert!(app.world().get::<Camera>(main).unwrap().is_active);
        assert!(app.world().get::<Camera>(overlay).unwrap().is_active);
        assert!(!app.world().get::<Camera>(inactive).unwrap().is_active);
        assert!(!app
            .world()
            .contains_resource::<super::super::surface_submission::TeachingRetirement>());
        assert!(!super::super::retire_teaching(&mut app));
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<CleanupExtractions>()
                .0,
            1,
            "repeated Close must not schedule another extraction"
        );
        // A delayed Close after a newer view binds must remain a no-op.
        let reopened = Arc::new(AtomicBool::new(false));
        bind_view(app.world_mut(), reopened);
        assert!(!super::super::retire_teaching(&mut app));
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<CleanupExtractions>()
                .0,
            1
        );
    }

    #[test]
    fn native_teaching_close_drops_resident_scene_but_keeps_live_assets_and_new_views() {
        let mut world = World::new();
        world.init_resource::<Scene>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        let live_mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::default()));
        let live_material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let live = world
            .spawn((
                Mesh3d(live_mesh.clone()),
                MeshMaterial3d(live_material.clone()),
                Camera::default(),
                NativeViewportCamera,
            ))
            .id();
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::default()));
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let geometry = world
            .spawn((
                TeachingOwned,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
            ))
            .id();
        let fixtures = (0..3)
            .map(|_| world.spawn(TeachingOwned).id())
            .collect::<Vec<_>>();
        let solid = Arc::new(SolidSceneDto::default());
        let witness = Arc::downgrade(&solid);
        {
            let mut scene = world.resource_mut::<Scene>();
            scene.geometry = vec![geometry];
            scene.fixtures = fixtures.clone();
            scene.meshes = vec![mesh];
            scene.materials = vec![material];
            scene.solid = Some(solid);
            scene.key = Some(("private package".into(), 0));
        }
        let old_view = Arc::new(AtomicBool::new(false));
        bind_view(&mut world, Arc::clone(&old_view));
        hide(&mut world);
        assert!(
            !release_closed(&mut world),
            "Pausing between frames must keep reusable geometry"
        );
        let current_view = Arc::new(AtomicBool::new(false));
        bind_view(&mut world, Arc::clone(&current_view));
        old_view.store(true, Ordering::Release);
        assert!(
            !release_closed(&mut world),
            "An old Close must not release a newer view"
        );
        current_view.store(true, Ordering::Release);
        assert!(release_closed(&mut world));
        assert!(witness.upgrade().is_none());
        assert!(world.get_entity(geometry).is_err());
        assert!(fixtures
            .into_iter()
            .all(|entity| world.get_entity(entity).is_err()));
        assert!(world.resource::<Scene>().key.is_none());
        assert!(world.resource::<Scene>().view_closed.is_none());
        assert_eq!(world.resource::<Assets<Mesh>>().len(), 1);
        assert_eq!(world.resource::<Assets<StandardMaterial>>().len(), 1);
        assert!(world.resource::<Assets<Mesh>>().contains(live_mesh.id()));
        assert!(world
            .resource::<Assets<StandardMaterial>>()
            .contains(live_material.id()));
        assert!(world.get::<Camera>(live).unwrap().is_active);
        assert!(
            !release_closed(&mut world),
            "Repeated cleanup must be a no-op"
        );
    }
}
