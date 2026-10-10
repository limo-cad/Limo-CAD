//! Disposable source-definition cutaway, independent of tools and print poses.
use super::*;
use limo_cad_occt::section_review::SectionReviewRequest;
use limo_cad_solid::KernelBodyDto;

/// Prepared on the query worker, including the bounds used by camera Fit.
pub(crate) struct PreparedCutaway {
    body: KernelBodyDto,
    bounds: Option<([f32; 3], [f32; 3])>,
}
impl PreparedCutaway {
    pub(crate) fn new(body: KernelBodyDto) -> Self {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in body.positions.as_chunks::<3>().0 {
            let p = Vec3::from_array(*p);
            min = min.min(p);
            max = max.max(p);
        }
        let bounds =
            (min.is_finite() && max.is_finite()).then_some((min.to_array(), max.to_array()));
        Self { body, bounds }
    }
}

struct Entry {
    session: String,
    revision: u64,
    request: SectionReviewRequest,
    cutaway: Arc<PreparedCutaway>,
}
#[derive(Resource, Default)]
pub(super) struct State {
    entry: Option<Entry>,
    generation: u64,
    rendered: u64,
}
impl State {
    pub(super) fn active(&self, model: &ModelResource) -> bool {
        !model.transient_model
            && self.entry.as_ref().is_some_and(|e| {
                e.session == model.session_id && e.revision == model.geometry_revision
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    #[test]
    fn cutaway_is_owned_retained_and_restores_source_visibility_and_assets() {
        let mut world = World::new();
        world.insert_resource(ModelResource {
            session_id: "section-test".into(),
            geometry_revision: 7,
            ..default()
        });
        world.init_resource::<State>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Assets<ReferencePlaneMaterial>>();
        world.init_resource::<PresentationResource>();
        world.init_resource::<PaletteResource>();
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let source = world
            .spawn((
                NativeCadBody {
                    body_id: 1,
                    occurrence_id: None,
                },
                NativeModelGeometry {
                    session_id: "section-test".into(),
                    geometry_revision: 7,
                    instance_revision: 0,
                },
                MeshMaterial3d(material),
                Visibility::Inherited,
            ))
            .id();
        let request: SectionReviewRequest =
            serde_json::from_value(serde_json::json!({"body_id":1,"plane":"xy","offset_mm":0}))
                .unwrap();
        let body = Arc::new(PreparedCutaway::new(KernelBodyDto {
            body_id: limo_cad_core::BodyId(1),
            topology_signature: String::new(),
            display_warnings: Vec::new(),
            positions: vec![0., 0., 0., 1., 0., 0., 0., 1., 0.],
            normals: vec![0., 0., 1., 0., 0., 1., 0., 0., 1.],
            indices: vec![0, 1, 2],
            faces: vec![limo_cad_solid::KernelFaceDto {
                linear_seam_edge_keys: Vec::new(),
                outer_shell: None,
                key: "cap".into(),
                first_index: 0,
                index_count: 3,
                plane: Some(PlaneBasis {
                    origin: [0.; 3],
                    u: [1., 0., 0.],
                    v: [0., 1., 0.],
                    normal: [0., 0., 1.],
                }),
                signature: None,
                cylinder: None,
                edge_keys: vec![],
                cone: None,
            }],
            edges: vec![],
        }));
        assert!(show(&mut world, "wrong-document", &request, Some(body.clone())).is_err());
        show(&mut world, "section-test", &request, Some(body.clone())).unwrap();
        world.run_system_once(rebuild).unwrap();
        world
            .run_system_once(apply_native_presentation_styles)
            .unwrap();
        assert_eq!(
            *world.get::<Visibility>(source).unwrap(),
            Visibility::Hidden
        );
        let count = world.resource::<Assets<Mesh>>().len();
        assert_eq!(count, 1);
        world.clear_trackers();
        show(&mut world, "section-test", &request, Some(body)).unwrap();
        assert!(!world.get_resource_ref::<State>().unwrap().is_changed());
        assert!(!world
            .get_resource_ref::<PresentationResource>()
            .unwrap()
            .is_changed());
        world.run_system_once(rebuild).unwrap();
        assert_eq!(world.resource::<Assets<Mesh>>().len(), count);
        assert_eq!(bounds(&world), Some(([0., 0., 0.], [1., 1., 0.])));
        world.resource_mut::<ModelResource>().geometry_revision = 8;
        assert!(!active(&world));
        world.run_system_once(rebuild).unwrap();
        world
            .run_system_once(apply_native_presentation_styles)
            .unwrap();
        assert_eq!(
            *world.get::<Visibility>(source).unwrap(),
            Visibility::Inherited
        );
        assert_eq!(world.resource::<Assets<Mesh>>().len(), 0);
        assert_eq!(world.resource::<Assets<StandardMaterial>>().len(), 1);
        world.clear_trackers();
        clear(&mut world);
        assert!(!world.get_resource_ref::<State>().unwrap().is_changed());
        assert!(!world
            .get_resource_ref::<PresentationResource>()
            .unwrap()
            .is_changed());
        assert!(world
            .resource::<PresentationResource>()
            .0
            .hidden_body_ids
            .is_empty());
    }
}
#[derive(Component)]
pub(super) struct SectionMesh;

pub(crate) fn show(
    world: &mut World,
    session: &str,
    request: &SectionReviewRequest,
    cutaway: Option<Arc<PreparedCutaway>>,
) -> Result<(), String> {
    let Some(model) = world.get_resource::<ModelResource>() else {
        return Ok(());
    };
    if model.session_id != session {
        return Err("Section belongs to another document".into());
    }
    let revision = model.geometry_revision;
    let cutaway = cutaway.filter(|cut| !cut.body.indices.is_empty());
    world.init_resource::<State>();
    let state = world.resource::<State>();
    if state.entry.as_ref().is_some_and(|e| {
        e.session == session
            && e.revision == revision
            && cutaway.as_ref().is_some_and(|b| Arc::ptr_eq(b, &e.cutaway))
    }) {
        return Ok(());
    }
    if state.entry.is_none() && cutaway.is_none() {
        return Ok(());
    }
    let mut state = world.resource_mut::<State>();
    state.entry = cutaway.map(|cutaway| Entry {
        session: session.into(),
        revision,
        request: request.clone(),
        cutaway,
    });
    state.generation = state.generation.wrapping_add(1);
    invalidate_interface_presentation(world);
    Ok(())
}
pub(crate) fn clear(world: &mut World) {
    if world
        .get_resource::<State>()
        .is_some_and(|state| state.entry.is_some())
    {
        let mut state = world.resource_mut::<State>();
        state.entry = None;
        state.generation = state.generation.wrapping_add(1);
        invalidate_interface_presentation(world);
    }
}
pub(crate) fn bounds(world: &World) -> Option<([f32; 3], [f32; 3])> {
    let state = world.get_resource::<State>()?;
    if !state.active(world.get_resource::<ModelResource>()?) {
        return None;
    }
    state.entry.as_ref()?.cutaway.bounds
}
pub(super) fn active(world: &World) -> bool {
    world
        .get_resource::<State>()
        .zip(world.get_resource::<ModelResource>())
        .is_some_and(|(s, m)| s.active(m))
}

pub(super) fn rebuild(
    mut commands: Commands,
    model: Res<ModelResource>,
    mut state: ResMut<State>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>), With<SectionMesh>>,
) {
    if state.entry.is_some() && !state.active(&model) {
        state.entry = None;
        state.generation = state.generation.wrapping_add(1);
    }
    if state.rendered == state.generation {
        return;
    }
    state.rendered = state.generation;
    for (e, m, s) in &existing {
        meshes.remove(m.0.id());
        materials.remove(s.0.id());
        commands.entity(e).despawn();
    }
    let Some(e) = &state.entry else {
        return;
    };
    let axis = e.request.plane.axis();
    let body = &e.cutaway.body;
    let mut surfaces = Vec::new();
    let mut caps = Vec::new();
    for face in &body.faces {
        let cap = face.plane.is_some_and(|p| {
            p.normal[axis].abs() > 0.999999 && (p.origin[axis] - e.request.offset_mm).abs() < 1e-6
        });
        let indices = &body.indices
            [face.first_index as usize..(face.first_index + face.index_count) as usize];
        if cap {
            caps.extend_from_slice(indices);
        } else {
            surfaces.extend_from_slice(indices);
        }
    }
    for (indices, color, name) in [
        (
            surfaces,
            Color::srgb(0.64, 0.74, 0.82),
            "OCCT section solid",
        ),
        (caps, Color::srgb(0.96, 0.57, 0.17), "OCCT section cap"),
    ] {
        if indices.is_empty() {
            continue;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            body.positions.as_chunks::<3>().0.to_vec(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            body.normals.as_chunks::<3>().0.to_vec(),
        );
        mesh.insert_indices(Indices::U32(indices));
        commands.spawn((
            Name::new(name),
            SectionMesh,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                double_sided: true,
                cull_mode: None,
                perceptual_roughness: 0.8,
                ..default()
            })),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}
pub(super) fn draw(
    model: Res<ModelResource>,
    state: Res<State>,
    mut gizmos: Gizmos<CadModelEdgeGizmos>,
) {
    if !state.active(&model) {
        return;
    }
    let e = state.entry.as_ref().unwrap();
    for edge in &e.cutaway.body.edges {
        for p in edge.points.windows(2) {
            gizmos.line(
                Vec3::new(p[0].x as f32, p[0].y as f32, p[0].z as f32),
                Vec3::new(p[1].x as f32, p[1].y as f32, p[1].z as f32),
                Color::srgb(0.16, 0.20, 0.24),
            );
        }
    }
}
