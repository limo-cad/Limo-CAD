use super::gpu_stock::{GpuStock, GpuStockInputs, GpuStockPlugin, GpuStockStamp};
use super::interface_shell::{self, NativeInterfaceHandle};
use super::path_progress::{active_cursor, split_segment};
use super::profile_outline::{base_curve_remainder, profile_outline_segments, BaseCurveRemainder};
use super::reference_planes::{ReferencePlaneMaterial, ReferencePlanePlugin};
use super::ui::{
    self, HudAxisLabel, HudAxisMark, NativeHudRoot, ViewportUiAssets, ViewportUiTheme,
};
use super::{
    NativePick, NativePickPurpose, ViewportAnnotationKind, ViewportCamStock, ViewportCamTool,
    ViewportCamera, ViewportHud, ViewportLinePattern, ViewportMode, ViewportModel,
    ViewportOriginPlane, ViewportPalette, ViewportPresentation, ViewportPreview, ViewportSnapKind,
    ViewportSnapMarker,
};
#[cfg(target_os = "linux")]
use bevy::render::settings::{RenderCreation, WgpuFeatures, WgpuSettings, WgpuSettingsPriority};
use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::RenderLayers, ClearColorConfig},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::Indices,
    prelude::*,
    render::{render_resource::PrimitiveTopology, RenderPlugin},
    text::FontWeight,
    ui::UiTransform,
};
#[cfg(test)]
use limo_cad_core::BodyAppearance;
use limo_cad_core::PlaneBasis;
use limo_cad_sketch::{BodyPoseDto, EntityDto, InstanceBodyPoseDto, SketchDto, Vec2 as SketchVec2};
use limo_cad_solid::{
    BodyDto, FaceDto, Point2Dto, ProfileLoopDto, SketchPointKindDto, SketchPointRefDto,
    SolidSceneDto,
};
#[cfg(test)]
use std::time::Instant;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

type CadGeometryQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static NativeModelGeometry,
        &'static mut Visibility,
        Option<&'static Mesh3d>,
        Option<&'static MeshMaterial3d<StandardMaterial>>,
        Option<&'static NativeCadBody>,
    ),
>;

type KeyLightQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut DirectionalLight),
    (
        With<CadKeyLight>,
        Without<CadFillLight>,
        Without<NativeViewportCamera>,
    ),
>;

type FillLightQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut DirectionalLight),
    (
        With<CadFillLight>,
        Without<CadKeyLight>,
        Without<NativeViewportCamera>,
    ),
>;

type DatumPlaneQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static NativeDatumPlane,
        &'static NativeModelGeometry,
        &'static mut Transform,
    ),
    (Without<NativeOriginPlane>, Without<NativeCadFace>),
>;

type FaceOverlayCache<'w> = Local<'w, Option<FaceOverlayStamp>>;

struct FaceOverlayStamp {
    session_id: String,
    geometry_revision: u64,
    instance_revision: u64,
    selected_faces: Vec<u64>,
    hovered_face: Option<u64>,
    selected_occurrence: Option<u64>,
    hovered_occurrence: Option<u64>,
    hidden_bodies: Vec<u64>,
    colors: [[f32; 3]; 2],
}

impl FaceOverlayStamp {
    fn matches(
        &self,
        model: &ModelResource,
        state: &ViewportPresentation,
        palette: ViewportPalette,
    ) -> bool {
        self.session_id == model.session_id
            && self.geometry_revision == model.geometry_revision
            && self.instance_revision == model.instance_revision
            && self.selected_faces == state.selected_face_ids
            && self.hovered_face == state.hovered_face_id
            && self.selected_occurrence == state.selected_occurrence_id
            && self.hovered_occurrence == state.hovered_occurrence_id
            && self.hidden_bodies == state.hidden_body_ids
            && self.colors == [palette.face_selected, palette.face_hover]
    }

    fn capture(
        model: &ModelResource,
        state: &ViewportPresentation,
        palette: ViewportPalette,
    ) -> Self {
        Self {
            session_id: model.session_id.clone(),
            geometry_revision: model.geometry_revision,
            instance_revision: model.instance_revision,
            selected_faces: state.selected_face_ids.clone(),
            hovered_face: state.hovered_face_id,
            selected_occurrence: state.selected_occurrence_id,
            hovered_occurrence: state.hovered_occurrence_id,
            hidden_bodies: state.hidden_body_ids.clone(),
            colors: [palette.face_selected, palette.face_hover],
        }
    }
}

type BodyPoseQuery<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static NativeCadBody>,
        Option<&'static NativeCadFace>,
        Option<&'static NativeCadFaceOverlay>,
        &'static mut Transform,
    ),
>;

#[path = "script_preview.rs"]
pub(crate) mod script_preview;
#[path = "section_view.rs"]
pub(crate) mod section_view;

pub(super) const VIEWPORT_MSAA: Msaa = Msaa::Sample4;
/// Base mesh size; a camera-aware transform keeps its screen footprint stable.
const REFERENCE_PLANE_HALF_SIZE: f32 = 50.0;
const REFERENCE_PLANE_SCREEN_FRACTION: f32 = 0.32;
const SKETCH_LINE_WIDTH: f32 = 1.25;
const FINISHED_SKETCH_OFFSET: f32 = 0.05;
const PROFILE_PICK_OFFSET: f32 = 0.08;
const DIRECT_PICK_FEEDBACK_OFFSET: f32 = PROFILE_PICK_OFFSET;
const PICK_FEEDBACK_LINE_WIDTH: f32 = 1.0;
const DIRECT_PICK_FEEDBACK_LINE_WIDTH: f32 = 0.75;
const PROFILE_BORDER_LINE_WIDTH: f32 = 0.75;
const PICK_FEEDBACK_DEPTH_BIAS: f32 = -0.98;
const PICK_FEEDBACK_HALO_LINE_WIDTH: f32 = 2.0;
const PICK_FEEDBACK_HALO_DEPTH_BIAS: f32 = -0.96;
const DIRECT_PICK_FEEDBACK_DEPTH_BIAS: f32 = -1.0;
const VIEWPORT_LINE_REFERENCE_DIAGONAL: f32 = 1200.0;
const VIEWPORT_LINE_SCALE_MIN: f32 = 0.9;
const VIEWPORT_LINE_SCALE_MAX: f32 = 1.6;
/// Below this projected radius, per-edge gizmos cost more CPU/GPU bandwidth
/// than the outline information they can convey. Bevy still renders the
/// retained shaded mesh and selected/hovered geometry always bypasses LOD.
const OCCURRENCE_EDGE_LOD_MIN_RADIUS_PX: f32 = 3.0;
/// A screen-sized or depth-range-relative lift can exceed a thin wall at wide
/// zooms and reveal hidden edges. Cap the tie-break in model units (0.1
/// micrometre), below modeling tolerances, regardless of zoom/display density.
const MODEL_EDGE_MAX_LIFT_MM: f32 = 1.0e-4;
/// Model-edge strokes are two pixels wide plus anti-aliasing, so their outer
/// pixels sit this far from the true edge on screen.
const MODEL_EDGE_STROKE_HALF_WIDTH_PX: f32 = 1.5;
/// Largest camera lift a stroke may take to clear a face that rises towards
/// the camera beside it, in pixels of depth. Beyond this the face is so
/// grazing that the stroke would float visibly in front of anything behind.
const MODEL_EDGE_MAX_LIFT_PX: f32 = 2.5;
/// The same lift as a fraction of the body's bounding radius: at wide zooms a
/// few pixels of depth are a real distance, and a lifted inside-corner edge
/// must never pass through the body's own walls.
const MODEL_EDGE_MAX_LIFT_BODY_FRACTION: f32 = 0.01;
const SKETCH_DEPTH_BIAS: f32 = -0.90;
const SKETCH_POINT_OUTLINE_WIDTH: f32 = 2.0;
const SKETCH_POINT_OUTLINE_DEPTH_BIAS: f32 = -0.89;
const SKETCH_POINT_RADIUS_PX: f32 = 2.5;
const SKETCH_POINT_OUTLINE_RADIUS_PX: f32 = 3.25;
const HIGHLIGHT_LINE_WIDTH: f32 = 2.0;
const SNAP_MARKER_HALF_SIZE_PX: f32 = 6.0;

struct PickState {
    scene: Arc<SolidSceneDto>,
    body_poses: Arc<Vec<BodyPoseDto>>,
    instance_body_poses: Arc<Vec<InstanceBodyPoseDto>>,
    camera: ViewportCamera,
    logical_size: (f32, f32),
    hidden_body_ids: Vec<u64>,
}

#[derive(Resource, Clone)]
struct SharedPickState(Arc<Mutex<PickState>>);

impl Default for PickState {
    fn default() -> Self {
        Self {
            scene: Arc::default(),
            body_poses: Arc::default(),
            instance_body_poses: Arc::default(),
            camera: ViewportCamera::default(),
            logical_size: (1.0, 1.0),
            hidden_body_ids: Vec::new(),
        }
    }
}

fn validate_preview(preview: &ViewportPreview) -> Result<(), String> {
    const MAX_LINE_FLOATS: usize = 6 * 65_536;
    const MAX_POINT_FLOATS: usize = 3 * 32_768;
    const MAX_TRIANGLE_FLOATS: usize = 9 * 65_536;
    const MAX_ARROWS: usize = 256;
    const MAX_ANNOTATIONS: usize = 2_048;
    let line_floats = preview
        .lines
        .iter()
        .map(|layer| layer.segments.len())
        .sum::<usize>();
    let point_floats = preview
        .points
        .iter()
        .map(|layer| layer.positions.len())
        .sum::<usize>();
    let triangle_floats = preview
        .triangles
        .iter()
        .map(|layer| layer.positions.len())
        .sum::<usize>();
    if preview.lines.len() > 128
        || preview.points.len() > 128
        || preview.triangles.len() > 128
        || preview.arrows.len() > MAX_ARROWS
        || line_floats > MAX_LINE_FLOATS
        || point_floats > MAX_POINT_FLOATS
        || triangle_floats > MAX_TRIANGLE_FLOATS
        || preview.annotations.len() > MAX_ANNOTATIONS
        || preview
            .annotations
            .iter()
            .any(|annotation| annotation.text.len() > 128)
    {
        return Err("native transient presentation is too large".to_string());
    }
    if preview.lines.iter().any(|layer| {
        layer
            .playback
            .as_ref()
            .is_some_and(|playback| !playback.is_valid_for(layer.segments.len()))
    }) {
        return Err("native timed path has invalid segment timing".to_string());
    }
    Ok(())
}

fn validate_cam_stock(stock: Option<&ViewportCamStock>) -> Result<(), String> {
    const MAX_CAM_STOCK_FLOATS: usize = 9 * 262_144;
    if let Some(stock) = stock {
        if stock.positions.is_empty()
            || !stock.positions.len().is_multiple_of(9)
            || stock.positions.len() > MAX_CAM_STOCK_FLOATS
            || (!stock.normals.is_empty() && stock.normals.len() != stock.positions.len())
            || !stock
                .positions
                .iter()
                .chain(stock.normals.iter())
                .all(|value| value.is_finite())
        {
            return Err("native CAM stock surface is invalid or too large".to_string());
        }
    }
    Ok(())
}

#[derive(Resource, Default)]
struct ModelResource {
    session_id: String,
    geometry_revision: u64,
    document: Arc<limo_cad_native_engine::NativeViewportDocument>,
    cache_entity: Option<Entity>,
    body_poses: Arc<Vec<BodyPoseDto>>,
    instance_body_poses: Arc<Vec<InstanceBodyPoseDto>>,
    instance_revision: u64,
    instance_states: HashMap<String, ModelInstanceState>,
    next_instance_revision: u64,
    revision: u64,
    transient_model: bool,
}

/// Cache incarnations belong to documents, not the order their tabs are visited.
/// Keep only occurrence identity/visibility; motion updates retain the meshes.
#[derive(Default)]
struct ModelInstanceState {
    revision: u64,
    layout: Vec<(u64, u64, u64, bool)>,
    transient_model: bool,
}

enum InstanceUpdate {
    LiveModel,
    IsolatedModel,
    Presentation,
}

impl ModelInstanceState {
    fn advance(&mut self, revision: &mut u64) {
        *revision = revision.wrapping_add(1);
        self.revision = *revision;
    }

    fn update_layout(&mut self, instances: &[InstanceBodyPoseDto], revision: &mut u64) {
        if self.layout.len() == instances.len()
            && self
                .layout
                .iter()
                .copied()
                .eq(instances.iter().map(instance_layout_key))
        {
            return;
        }
        self.layout = instances.iter().map(instance_layout_key).collect();
        self.advance(revision);
    }

    fn replace_model(
        &mut self,
        instances: &[InstanceBodyPoseDto],
        transient_model: bool,
        revision: &mut u64,
    ) {
        self.update_layout(instances, revision);
        if self.transient_model || transient_model {
            self.advance(revision);
        }
        self.transient_model = transient_model;
    }
}

impl ModelResource {
    fn bind_instance_state(
        &mut self,
        session_id: &str,
        instances: &[InstanceBodyPoseDto],
        update: InstanceUpdate,
    ) {
        let state = self
            .instance_states
            .entry(session_id.into())
            .or_insert_with(|| {
                let mut state = ModelInstanceState::default();
                state.advance(&mut self.next_instance_revision);
                state
            });
        match update {
            InstanceUpdate::LiveModel => {
                state.replace_model(instances, false, &mut self.next_instance_revision);
            }
            InstanceUpdate::IsolatedModel => {
                state.replace_model(instances, true, &mut self.next_instance_revision);
            }
            InstanceUpdate::Presentation => {
                state.update_layout(instances, &mut self.next_instance_revision);
            }
        }
        self.instance_revision = state.revision;
        self.transient_model = state.transient_model;
    }
}

#[derive(Resource, Default)]
struct DocumentGeometryIndex(HashMap<String, Entity>);

/// Local geometry metadata belongs to its document and survives tab switches.
/// Rigid placements and occurrence layout do not change this data.
#[derive(Component, Default)]
struct ModelEdgeCache {
    scene: std::sync::Weak<SolidSceneDto>,
    bodies: HashMap<u64, BodyEdgeMetadata>,
}

/// Strong mesh handles retain one upload per body across occurrence changes.
#[derive(Component, Default)]
struct BodyMeshCache {
    scene: std::sync::Weak<SolidSceneDto>,
    bodies: HashMap<u64, Handle<Mesh>>,
    rendered: Option<(u64, u64)>,
}

struct BodyEdgeMetadata {
    local_bounds: Option<(Vec3, f32)>,
    sides: Vec<[Option<EdgeSideFace>; 2]>,
    face_boundaries: HashMap<u64, Arc<Vec<(Vec3, Vec3)>>>,
}

impl ModelEdgeCache {
    fn update(&mut self, scene: &Arc<SolidSceneDto>) {
        if std::ptr::eq(self.scene.as_ptr(), Arc::as_ptr(scene)) {
            return;
        }
        self.bodies = scene
            .bodies
            .iter()
            .map(|body| {
                let sides = edge_side_faces(body, &Transform::IDENTITY);
                (
                    body.id.0,
                    BodyEdgeMetadata {
                        local_bounds: body_local_bounding_sphere(body),
                        sides: body
                            .edges
                            .iter()
                            .map(|edge| sides.get(edge.key.as_str()).copied().unwrap_or_default())
                            .collect(),
                        face_boundaries: body
                            .faces
                            .iter()
                            .map(|face| (face.id.0, Arc::new(face_boundary_segments(body, face))))
                            .collect(),
                    },
                )
            })
            .collect();
        self.scene = Arc::downgrade(scene);
    }
}

#[derive(Resource)]
struct CameraResource {
    camera: ViewportCamera,
    revision: u64,
}

#[derive(Resource, Default)]
struct PreviewResource {
    value: Arc<ViewportPreview>,
    /// Native sketch dimensions persist while transient creation/tool guides
    /// change. They never replace or take ownership of a feature preview.
    sketch_lines: Vec<super::ViewportLineLayer>,
    revision: u64,
    /// Changes only when GPU mesh content changes. Screen annotations and
    /// sketch gizmos may update at camera frequency without reallocating the
    /// retained profile/tool meshes.
    mesh_revision: u64,
}

#[derive(Resource, Default)]
struct CamStockResource {
    value: Option<ViewportCamStock>,
    revision: u64,
}

#[derive(Resource, Clone, Copy, Default)]
struct PaletteResource(ViewportPalette);

#[derive(Resource)]
struct HudResource {
    hud: ViewportHud,
    revision: u64,
}

#[derive(Resource, Clone, Copy, PartialEq)]
struct ViewportSizeResource {
    logical_width: f32,
    logical_height: f32,
}

impl Default for ViewportSizeResource {
    fn default() -> Self {
        Self {
            logical_width: 1.0,
            logical_height: 1.0,
        }
    }
}

#[derive(Resource, Default)]
struct PresentationResource(ViewportPresentation);

impl Default for HudResource {
    fn default() -> Self {
        Self {
            hud: ViewportHud::default(),
            revision: 1,
        }
    }
}

impl Default for CameraResource {
    fn default() -> Self {
        Self {
            camera: ViewportCamera::default(),
            revision: 1,
        }
    }
}

#[derive(Resource, Default)]
struct RenderedRevisions {
    model: u64,
    camera: u64,
    hud: u64,
    annotations: u64,
    preview_meshes: u64,
    cam_stock: u64,
}

#[derive(Component)]
struct NativeCadBody {
    body_id: u64,
    occurrence_id: Option<u64>,
}

#[derive(Component)]
struct NativeCadFace {
    body_id: u64,
    occurrence_id: Option<u64>,
    face_id: u64,
    boundary: Arc<Vec<(Vec3, Vec3)>>,
}

#[derive(Component)]
struct NativeCadFaceOverlay {
    body_id: u64,
    occurrence_id: Option<u64>,
}

#[derive(Component)]
struct NativeModelGeometry {
    session_id: String,
    geometry_revision: u64,
    instance_revision: u64,
}

#[derive(Component)]
struct NativeCadCamera;

/// Common marker for the model camera and the depth-independent transient
/// overlay camera. Both always receive exactly the same projection.
#[derive(Component)]
struct NativeViewportCamera;

#[derive(Component)]
struct NativeOverlayCamera;

#[derive(Component)]
struct NativePreviewMesh;

#[derive(Component)]
struct NativeCamStockMesh;

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeCamToolPartKind {
    Flute,
    Shank,
}

#[derive(Component, Clone, Copy)]
struct NativeCamToolPart {
    kind: NativeCamToolPartKind,
    geometry: limo_cad_cam::CamCutterGeometryDto,
}

#[derive(Clone, Copy)]
enum NativePreviewArrowPartKind {
    Shaft,
    Head,
    Base,
}

/// Semantic arrow data retained on each unit primitive. Camera movement only
/// updates these transforms; it never allocates a new Mesh or Material.
#[derive(Component, Clone, Copy)]
struct NativePreviewArrowPart {
    start: Vec3,
    end: Vec3,
    width: f32,
    kind: NativePreviewArrowPartKind,
}

#[derive(Component)]
struct CadKeyLight;

#[derive(Component)]
struct CadFillLight;

#[derive(Component)]
struct NativeDatumPlane {
    datum_id: u64,
}

#[derive(Component, Clone, Copy)]
struct NativeOriginPlane {
    plane: ViewportOriginPlane,
}

#[derive(Component)]
struct NativeAnnotationRoot;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadHighlightGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadModelEdgeGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CamCompletedPathGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadSketchGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadPickFeedbackGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadDirectPickFeedbackGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadProfileBorderGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadPickFeedbackHaloGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadSketchPointOutlineGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct CadSketchPointGizmos;

pub(super) fn cad_render_plugin() -> RenderPlugin {
    let render_plugin = RenderPlugin {
        synchronous_pipeline_compilation: true,
        ..default()
    };
    #[cfg(target_os = "linux")]
    let render_plugin = RenderPlugin {
        render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
            priority: WgpuSettingsPriority::WebGPU,
            features: WgpuFeatures::empty(),
            ..default()
        })),
        ..render_plugin
    };
    render_plugin
}

/// The application viewport and immutable script previews run these exact systems
/// in separate worlds. A preview never replaces the live model or camera.
pub(super) fn install_cad_scene(app: &mut bevy::app::App) {
    app.init_gizmo_group::<CadHighlightGizmos>()
        .init_gizmo_group::<CadModelEdgeGizmos>()
        .init_gizmo_group::<CamCompletedPathGizmos>()
        .init_gizmo_group::<CadSketchGizmos>()
        .init_gizmo_group::<CadPickFeedbackHaloGizmos>()
        .init_gizmo_group::<CadPickFeedbackGizmos>()
        .init_gizmo_group::<CadDirectPickFeedbackGizmos>()
        .init_gizmo_group::<CadProfileBorderGizmos>()
        .init_gizmo_group::<CadSketchPointOutlineGizmos>()
        .init_gizmo_group::<CadSketchPointGizmos>();
    let initial_palette = ViewportPalette::default();
    app.insert_resource(ClearColor(rgb(initial_palette.background)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            brightness: 900.0,
            ..default()
        })
        .init_resource::<ModelResource>()
        .init_resource::<DocumentGeometryIndex>()
        .init_resource::<CameraResource>()
        .init_resource::<PreviewResource>()
        .init_resource::<section_view::State>()
        .init_resource::<CamStockResource>()
        .init_resource::<PaletteResource>()
        .init_resource::<HudResource>()
        .init_resource::<ViewportSizeResource>()
        .init_resource::<PresentationResource>()
        .init_resource::<RenderedRevisions>()
        .init_resource::<ViewportUiAssets>()
        .add_plugins((GpuStockPlugin, ReferencePlanePlugin))
        .add_systems(
            Startup,
            (ui::load_system_font, setup_gpu_stock, setup_scene).chain(),
        )
        .add_systems(
            Update,
            (
                rebuild_occt_meshes,
                section_view::rebuild,
                apply_camera,
                resize_reference_planes,
                apply_native_presentation_styles,
                rebuild_native_face_overlays,
                apply_body_poses,
                rebuild_native_preview_meshes,
                rebuild_native_cam_stock,
                update_native_cam_stock_visibility,
                update_native_cam_tool,
                update_gpu_stock,
                update_native_preview_arrows,
                rebuild_native_annotations,
                rebuild_native_hud,
                update_native_hud_orientation,
                draw_cad_gizmos,
                section_view::draw,
            )
                .chain()
                .after(interface_shell::InterfaceReduction),
        );
}

#[derive(Resource)]
struct SceneImageTarget(Handle<Image>);

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ReferencePlaneMaterial>>,
    mut gizmo_config: ResMut<GizmoConfigStore>,
    target: Option<Res<SceneImageTarget>>,
) {
    configure_viewport_line_widths(
        &mut gizmo_config,
        VIEWPORT_LINE_REFERENCE_DIAGONAL * 0.8,
        VIEWPORT_LINE_REFERENCE_DIAGONAL * 0.6,
        1.0,
    );
    gizmo_config.config_mut::<CadModelEdgeGizmos>().0.depth_bias = 0.0;
    let (highlight_config, _) = gizmo_config.config_mut::<CadHighlightGizmos>();
    highlight_config.depth_bias = -1.0;

    gizmo_config
        .config_mut::<CamCompletedPathGizmos>()
        .0
        .depth_bias = -0.995;

    let (sketch_config, _) = gizmo_config.config_mut::<CadSketchGizmos>();

    sketch_config.depth_bias = SKETCH_DEPTH_BIAS;
    let (pick_feedback_config, _) = gizmo_config.config_mut::<CadPickFeedbackGizmos>();
    pick_feedback_config.depth_bias = PICK_FEEDBACK_DEPTH_BIAS;
    let (direct_pick_feedback_config, _) = gizmo_config.config_mut::<CadDirectPickFeedbackGizmos>();
    direct_pick_feedback_config.depth_bias = DIRECT_PICK_FEEDBACK_DEPTH_BIAS;
    let (profile_border_config, _) = gizmo_config.config_mut::<CadProfileBorderGizmos>();
    profile_border_config.depth_bias = PICK_FEEDBACK_DEPTH_BIAS;
    let (pick_halo_config, _) = gizmo_config.config_mut::<CadPickFeedbackHaloGizmos>();
    pick_halo_config.depth_bias = PICK_FEEDBACK_HALO_DEPTH_BIAS;
    let (point_outline_config, _) = gizmo_config.config_mut::<CadSketchPointOutlineGizmos>();
    point_outline_config.depth_bias = SKETCH_POINT_OUTLINE_DEPTH_BIAS;
    let (point_config, _) = gizmo_config.config_mut::<CadSketchPointGizmos>();
    point_config.depth_bias = SKETCH_DEPTH_BIAS;

    let camera = ViewportCamera::default();
    let (key_transform, fill_transform) = camera_relative_light_transforms(camera);
    let model_camera = commands
        .spawn((
            Name::new("CAD camera"),
            NativeViewportCamera,
            NativeCadCamera,
            Camera3d::default(),
            VIEWPORT_MSAA,
            BoxShadowSamples(6),
            Projection::Perspective(PerspectiveProjection {
                fov: camera.vertical_fov_degrees.to_radians(),
                near: 0.1,
                far: 20_000.0,
                ..default()
            }),
            camera_transform(camera),
        ))
        .id();

    let overlay_camera = commands
        .spawn((
            Name::new("CAD transient overlay camera"),
            NativeViewportCamera,
            NativeOverlayCamera,
            IsDefaultUiCamera,
            Camera3d::default(),
            VIEWPORT_MSAA,
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: camera.vertical_fov_degrees.to_radians(),
                near: 0.1,
                far: 20_000.0,
                ..default()
            }),
            camera_transform(camera),
            RenderLayers::layer(1),
        ))
        .id();
    if let Some(target) = target {
        for camera in [model_camera, overlay_camera] {
            commands
                .entity(camera)
                .insert(bevy::camera::RenderTarget::Image(target.0.clone().into()));
        }
    }

    commands.spawn((
        Name::new("CAD key light"),
        CadKeyLight,
        DirectionalLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            illuminance: 2_200.0,
            shadow_maps_enabled: false,
            ..default()
        },
        key_transform,
    ));
    commands.spawn((
        Name::new("CAD fill light"),
        CadFillLight,
        DirectionalLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            illuminance: 2_200.0,
            shadow_maps_enabled: false,
            ..default()
        },
        fill_transform,
    ));

    for (name, basis, color) in origin_plane_bases() {
        let plane = match name {
            "XY" => ViewportOriginPlane::Xy,
            "XZ" => ViewportOriginPlane::Xz,
            _ => ViewportOriginPlane::Yz,
        };
        commands.spawn((
            Name::new(format!("Origin plane {name}")),
            NativeOriginPlane { plane },
            Visibility::Hidden,
            Mesh3d(meshes.add(reference_plane_mesh(&basis, REFERENCE_PLANE_HALF_SIZE))),
            MeshMaterial3d(materials.add(ReferencePlaneMaterial {
                color: color.to_linear(),
            })),
        ));
    }
}

fn rebuild_occt_meshes(
    mut commands: Commands,
    (model, mut revisions): (Res<ModelResource>, ResMut<RenderedRevisions>),
    mut body_caches: Query<(&mut BodyMeshCache, &ModelEdgeCache)>,
    mut existing: CadGeometryQuery,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    palette: Res<PaletteResource>,
) {
    if revisions.model == model.revision {
        return;
    }
    revisions.model = model.revision;
    let Some((mut body_cache, edge_cache)) = model
        .cache_entity
        .and_then(|entity| body_caches.get_mut(entity).ok())
    else {
        return;
    };
    let scene = &model.document.scene;
    let geometry_changed = !std::ptr::eq(body_cache.scene.as_ptr(), Arc::as_ptr(scene));
    if geometry_changed {
        for handle in body_cache.bodies.values() {
            meshes.remove(handle);
        }
        body_cache.bodies.clear();
        body_cache.scene = Arc::downgrade(scene);
        body_cache.rendered = None;
    }
    for (entity, geometry, mut visibility, mesh, material, body) in &mut existing {
        if geometry.session_id == model.session_id {
            if geometry_changed
                || geometry.geometry_revision != model.geometry_revision
                || geometry.instance_revision != model.instance_revision
            {
                if let Some(mesh) = mesh.filter(|_| body.is_none()) {
                    meshes.remove(&mesh.0);
                }
                if let Some(material) = material {
                    materials.remove(&material.0);
                }
                commands.entity(entity).despawn();
            }
        } else {
            *visibility = Visibility::Hidden;
        }
    }

    let cache_key = (model.geometry_revision, model.instance_revision);
    if body_cache.rendered == Some(cache_key) {
        return;
    }
    body_cache.rendered = Some(cache_key);

    for body in &model.document.scene.bodies {
        let instances = if model.instance_body_poses.is_empty() {
            vec![None]
        } else {
            model
                .instance_body_poses
                .iter()
                .filter(|instance| instance.body_id == body.id && instance.visible)
                .map(|instance| Some(instance.occurrence_id.0))
                .collect::<Vec<_>>()
        };
        if instances.is_empty() {
            continue;
        }
        let mesh_handle = if let Some(handle) = body_cache.bodies.get(&body.id.0) {
            Some(handle.clone())
        } else {
            body_mesh(body).map(|mesh| {
                let handle = meshes.add(mesh);
                body_cache.bodies.insert(body.id.0, handle.clone());
                handle
            })
        };
        for occurrence_id in instances {
            let transform = instance_body_pose_transform(
                &model.instance_body_poses,
                &model.body_poses,
                body.id.0,
                occurrence_id,
            );
            if let Some(mesh_handle) = &mesh_handle {
                commands.spawn((
                    Name::new(format!(
                        "OCCT body {} occurrence {:?} ({})",
                        body.id.0, occurrence_id, body.name
                    )),
                    NativeCadBody {
                        body_id: body.id.0,
                        occurrence_id,
                    },
                    NativeModelGeometry {
                        session_id: model.session_id.clone(),
                        geometry_revision: model.geometry_revision,
                        instance_revision: model.instance_revision,
                    },
                    Mesh3d(mesh_handle.clone()),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: body_appearance_color(&model, body.id.0, palette.0.body),
                        metallic: 0.0,
                        perceptual_roughness: 0.86,
                        cull_mode: None,
                        ..default()
                    })),
                    transform,
                ));
            }
            for face in &body.faces {
                commands.spawn((
                    Name::new(format!(
                        "OCCT face metadata {} on {} occurrence {:?} ({})",
                        face.id.0, body.id.0, occurrence_id, body.name
                    )),
                    NativeCadFace {
                        body_id: body.id.0,
                        occurrence_id,
                        face_id: face.id.0,
                        boundary: Arc::clone(
                            &edge_cache.bodies[&body.id.0].face_boundaries[&face.id.0],
                        ),
                    },
                    NativeModelGeometry {
                        session_id: model.session_id.clone(),
                        geometry_revision: model.geometry_revision,
                        instance_revision: model.instance_revision,
                    },
                    Visibility::Inherited,
                    transform,
                ));
            }
        }
    }

    for plane in &model.document.datum_planes {
        commands.spawn((
            Name::new(format!("Construction plane {}", plane.name)),
            NativeDatumPlane {
                datum_id: plane.datum_id.0,
            },
            NativeModelGeometry {
                session_id: model.session_id.clone(),
                geometry_revision: model.geometry_revision,
                instance_revision: model.instance_revision,
            },
            Mesh3d(meshes.add(reference_plane_mesh(
                &plane.basis,
                REFERENCE_PLANE_HALF_SIZE,
            ))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(0.85, 0.65, 0.30, 0.08),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                ..default()
            })),
        ));
    }
}

fn apply_camera(
    camera: Res<CameraResource>,
    image_target: Option<Res<SceneImageTarget>>,
    (presentation, mut ambient): (Res<PresentationResource>, ResMut<GlobalAmbientLight>),
    mut revisions: ResMut<RenderedRevisions>,
    mut query: Query<(&mut Transform, &mut Projection), With<NativeViewportCamera>>,
    mut key_lights: KeyLightQuery,
    mut fill_lights: FillLightQuery,
) {
    let cam_lighting = presentation.0.cam_stock_visible;
    let ambient_brightness = if cam_lighting { 500.0 } else { 350.0 };
    if revisions.camera == camera.revision && ambient.brightness == ambient_brightness {
        return;
    }
    revisions.camera = camera.revision;
    ambient.brightness = ambient_brightness;
    for (mut transform, mut projection) in &mut query {
        *transform = camera_transform(camera.camera);
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = camera.camera.vertical_fov_degrees.to_radians();
            let distance = Vec3::from_array(camera.camera.position)
                .distance(Vec3::from_array(camera.camera.target));
            if image_target.is_some() {
                perspective.near = (distance / 100_000.0).max(0.001);
                perspective.far = (distance * 3.0).max(100.0);
            } else {
                perspective.near = (distance / 100_000.0).clamp(0.000001, 0.1);
                perspective.far = (distance * 3.0).max(20_000.0);
            }
        }
    }
    let (key_transform, fill_transform) = if cam_lighting {
        cam_light_transforms(camera.camera)
    } else {
        camera_relative_light_transforms(camera.camera)
    };
    for (mut transform, mut light) in &mut key_lights {
        *transform = key_transform;
        light.illuminance = if cam_lighting { 2_600.0 } else { 3_200.0 };
    }
    for (mut transform, mut light) in &mut fill_lights {
        *transform = fill_transform;
        light.illuminance = if cam_lighting { 750.0 } else { 650.0 };
    }
}

/// An oblique key and softer fill reveal concave drill tips without an extra
/// shadow/SSAO pass. Only remaining-stock inspection uses this rig; the CAD
/// workspace retains its original symmetric studio lighting.
pub(super) fn cam_light_transforms(camera: ViewportCamera) -> (Transform, Transform) {
    let target = Vec3::from_array(camera.target);
    let view = (Vec3::from_array(camera.position) - target)
        .try_normalize()
        .unwrap_or(Vec3::Y);
    let up = Vec3::from_array(camera.up)
        .try_normalize()
        .unwrap_or(Vec3::Z);
    let right = view
        .cross(up)
        .try_normalize()
        .unwrap_or_else(|| view.any_orthonormal_vector());
    let key = target + (view + right * 0.85 + up * 0.2).normalize_or_zero() * 100.0;
    let fill = target + (view - right * 0.4).normalize_or_zero() * 100.0;
    (
        Transform::from_translation(key).looking_at(target, stable_view_up(target - key, up)),
        Transform::from_translation(fill).looking_at(target, stable_view_up(target - fill, up)),
    )
}

pub(super) fn cam_stock_material() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(0.16, 0.6, 0.25),
        alpha_mode: AlphaMode::Opaque,
        metallic: 0.0,
        perceptual_roughness: 0.68,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

/// Camera-relative key and softer fill make neighboring CAD faces distinct
/// while keeping illumination stable as the view orbits.
fn camera_relative_light_transforms(camera: ViewportCamera) -> (Transform, Transform) {
    let target = Vec3::from_array(camera.target);
    let eye = Vec3::from_array(camera.position);
    let view = (eye - target).normalize_or_zero();
    let view = if view.length_squared() < 1.0e-8 {
        Vec3::Y
    } else {
        view
    };
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let up_hint = if up_hint.length_squared() < 1.0e-8 {
        Vec3::Z
    } else {
        up_hint
    };
    let right = view.cross(up_hint).normalize_or_zero();
    let right = if right.length_squared() < 1.0e-8 {
        view.any_orthonormal_vector()
    } else {
        right
    };
    let rig_distance = eye.distance(target).max(100.0);
    let key_position = target + (view + right * 0.85 + up_hint * 0.65).normalize() * rig_distance;
    let fill_position = target + (view - right * 0.6).normalize() * rig_distance;
    (
        Transform::from_translation(key_position)
            .looking_at(target, stable_view_up(target - key_position, up_hint)),
        Transform::from_translation(fill_position)
            .looking_at(target, stable_view_up(target - fill_position, up_hint)),
    )
}

fn world_per_pixel_at(camera: ViewportCamera, viewport: ViewportSizeResource, origin: Vec3) -> f32 {
    let position = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - position).normalize_or_zero();
    let depth = (origin - position).dot(forward).max(0.2);
    let height = viewport.logical_height.max(1.0);
    2.0 * depth * (camera.vertical_fov_degrees.to_radians() * 0.5).tan() / height
}

fn viewport_line_logical_scale(logical_width: f32, logical_height: f32) -> f32 {
    if !logical_width.is_finite()
        || !logical_height.is_finite()
        || logical_width <= 0.0
        || logical_height <= 0.0
    {
        return 1.0;
    }
    (logical_width.hypot(logical_height) / VIEWPORT_LINE_REFERENCE_DIAGONAL)
        .clamp(VIEWPORT_LINE_SCALE_MIN, VIEWPORT_LINE_SCALE_MAX)
}

fn viewport_line_raster_scale(logical_width: f32, logical_height: f32, backing_scale: f32) -> f32 {
    viewport_line_logical_scale(logical_width, logical_height)
        * if backing_scale.is_finite() {
            backing_scale.max(0.5)
        } else {
            1.0
        }
}

fn configure_viewport_line_widths(
    gizmo_config: &mut GizmoConfigStore,
    logical_width: f32,
    logical_height: f32,
    backing_scale: f32,
) {
    let scale = viewport_line_raster_scale(logical_width, logical_height, backing_scale);
    gizmo_config.config_mut::<CadHighlightGizmos>().0.line.width = HIGHLIGHT_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CamCompletedPathGizmos>()
        .0
        .line
        .width = HIGHLIGHT_LINE_WIDTH * scale;
    gizmo_config.config_mut::<CadSketchGizmos>().0.line.width = SKETCH_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadPickFeedbackGizmos>()
        .0
        .line
        .width = PICK_FEEDBACK_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadDirectPickFeedbackGizmos>()
        .0
        .line
        .width = DIRECT_PICK_FEEDBACK_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadProfileBorderGizmos>()
        .0
        .line
        .width = PROFILE_BORDER_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadPickFeedbackHaloGizmos>()
        .0
        .line
        .width = PICK_FEEDBACK_HALO_LINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadSketchPointOutlineGizmos>()
        .0
        .line
        .width = SKETCH_POINT_OUTLINE_WIDTH * scale;
    gizmo_config
        .config_mut::<CadSketchPointGizmos>()
        .0
        .line
        .width = SKETCH_LINE_WIDTH * scale;
}

fn reference_plane_half_size(
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    origin: Vec3,
) -> f32 {
    world_per_pixel_at(camera, viewport, origin)
        * viewport.logical_width.min(viewport.logical_height).max(1.0)
        * (REFERENCE_PLANE_SCREEN_FRACTION * 0.5)
}

fn reference_plane_transform(origin: Vec3, half_size: f32) -> Transform {
    let scale = (half_size / REFERENCE_PLANE_HALF_SIZE).max(1.0e-6);
    Transform::from_translation(origin * (1.0 - scale)).with_scale(Vec3::splat(scale))
}

fn body_local_bounding_sphere(body: &BodyDto) -> Option<(Vec3, f32)> {
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut count = 0usize;
    for point in body.mesh.positions.as_chunks::<3>().0 {
        let point = Vec3::new(point[0], point[1], point[2]);
        minimum = minimum.min(point);
        maximum = maximum.max(point);
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let center = (minimum + maximum) * 0.5;
    let radius = (maximum - center).length().max(1.0e-5);
    Some((center, radius))
}

fn occurrence_edges_are_visible(
    local_bounds: Option<(Vec3, f32)>,
    transform: &Transform,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
) -> bool {
    let Some((local_center, local_radius)) = local_bounds else {
        return true;
    };
    let center = transform.transform_point(local_center);
    let radius = local_radius * transform.scale.max_element().abs().max(1.0e-6);
    let eye = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - eye).normalize_or_zero();
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let right = forward.cross(up_hint).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    if forward == Vec3::ZERO || right == Vec3::ZERO || up == Vec3::ZERO {
        return true;
    }
    let delta = center - eye;
    let depth = delta.dot(forward);
    if depth + radius <= 0.0 {
        return false;
    }
    let tangent = (camera.vertical_fov_degrees.to_radians() * 0.5).tan();
    let aspect = viewport.logical_width.max(1.0) / viewport.logical_height.max(1.0);
    if delta.dot(right).abs() > depth.max(0.0) * tangent * aspect + radius
        || delta.dot(up).abs() > depth.max(0.0) * tangent + radius
    {
        return false;
    }
    radius / world_per_pixel_at(camera, viewport, center) >= OCCURRENCE_EDGE_LOD_MIN_RADIUS_PX
}

fn resize_reference_planes(
    camera: Res<CameraResource>,
    viewport: Res<ViewportSizeResource>,
    model: Res<ModelResource>,
    mut origin_planes: Query<&mut Transform, (With<NativeOriginPlane>, Without<NativeDatumPlane>)>,
    mut datum_planes: DatumPlaneQuery,
) {
    if !camera.is_changed() && !viewport.is_changed() && !model.is_changed() {
        return;
    }
    let size = *viewport;
    let origin_half_size = reference_plane_half_size(camera.camera, size, Vec3::ZERO);
    for mut transform in &mut origin_planes {
        transform.set_if_neq(reference_plane_transform(Vec3::ZERO, origin_half_size));
    }
    for (plane, geometry, mut transform) in &mut datum_planes {
        if geometry.session_id != model.session_id {
            continue;
        }
        let Some(definition) = model
            .document
            .datum_planes
            .iter()
            .find(|candidate| candidate.datum_id.0 == plane.datum_id)
        else {
            continue;
        };
        let origin = basis_vector(definition.basis.origin);
        let half_size = reference_plane_half_size(camera.camera, size, origin);
        transform.set_if_neq(reference_plane_transform(origin, half_size));
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply_native_presentation_styles(
    (model, section): (Res<ModelResource>, Option<Res<section_view::State>>),
    (presentation, palette): (Res<PresentationResource>, Res<PaletteResource>),
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut plane_materials: ResMut<Assets<ReferencePlaneMaterial>>,
    mut bodies: Query<
        (
            &NativeCadBody,
            &NativeModelGeometry,
            &MeshMaterial3d<StandardMaterial>,
            &mut Visibility,
        ),
        (
            Without<NativeCadFace>,
            Without<NativeCadFaceOverlay>,
            Without<NativeDatumPlane>,
            Without<NativeOriginPlane>,
        ),
    >,
    mut datum_planes: Query<
        (
            &NativeDatumPlane,
            &NativeModelGeometry,
            &MeshMaterial3d<StandardMaterial>,
            &mut Visibility,
        ),
        (Without<NativeCadFace>, Without<NativeOriginPlane>),
    >,
    mut origin_planes: Query<
        (
            &NativeOriginPlane,
            &MeshMaterial3d<ReferencePlaneMaterial>,
            &mut Visibility,
        ),
        (Without<NativeCadFace>, Without<NativeDatumPlane>),
    >,
) {
    if !model.is_changed()
        && !presentation.is_changed()
        && !palette.is_changed()
        && section.as_ref().is_none_or(|s| !s.is_changed())
    {
        return;
    }
    let state = &presentation.0;
    for (body, geometry, handle, mut visibility) in &mut bodies {
        if geometry.session_id != model.session_id {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        visibility.set_if_neq(
            if section.as_ref().is_some_and(|s| s.active(&model))
                || state.hidden_body_ids.contains(&body.body_id)
            {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            },
        );
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        let occurrence_is_selected = state
            .selected_occurrence_id
            .is_none_or(|occurrence_id| body.occurrence_id == Some(occurrence_id));
        let selected_body_index = (occurrence_is_selected
            && state.selected_face_ids.is_empty()
            && state.selected_edge_ids.is_empty())
        .then(|| {
            state
                .selected_body_ids
                .iter()
                .position(|body_id| *body_id == body.body_id)
        })
        .flatten();
        let color = if selected_body_index == Some(0) {
            rgb(palette.0.body_selected)
        } else if selected_body_index.is_some() {
            rgb(palette.0.body_tool)
        } else {
            body_appearance_color(&model, body.body_id, palette.0.body)
        };

        let ghosted = state.ghosted_body_ids.contains(&body.body_id)
            || model
                .document
                .active_sketch
                .as_ref()
                .and_then(|sketch| sketch.edit_occurrence_id)
                .is_some_and(|editing| body.occurrence_id != Some(editing.0));
        let (base_color, alpha_mode, emissive) = if ghosted {
            (color.with_alpha(0.1), AlphaMode::Blend, LinearRgba::BLACK)
        } else {
            let emissive = if selected_body_index.is_some() {
                color.to_linear() * 0.08
            } else {
                LinearRgba::BLACK
            };
            (color, AlphaMode::Opaque, emissive)
        };
        if material.base_color != base_color
            || material.alpha_mode != alpha_mode
            || material.emissive != emissive
        {
            material.base_color = base_color;
            material.alpha_mode = alpha_mode;
            material.emissive = emissive;
        }
    }

    for (plane, geometry, handle, mut visibility) in &mut datum_planes {
        if geometry.session_id != model.session_id {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        visibility.set_if_neq(if state.hidden_datum_plane_ids.contains(&plane.datum_id) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
        if let Some(mut material) = materials.get_mut(&handle.0) {
            let hovered = state.hovered_datum_plane_id == Some(plane.datum_id);
            let selected = state.selected_datum_plane_id == Some(plane.datum_id);
            let color = if selected {
                palette.0.selection
            } else if hovered {
                palette.0.hover
            } else if state.mode == ViewportMode::PickPlane {
                palette.0.active_sketch
            } else {
                [0.85, 0.65, 0.30]
            };
            let base_color = rgba(
                color,
                if selected {
                    0.30
                } else if hovered {
                    0.24
                } else if state.mode == ViewportMode::PickPlane {
                    0.10
                } else {
                    0.08
                },
            );
            if material.base_color != base_color {
                material.base_color = base_color;
            }
        }
    }

    for (plane, handle, mut visibility) in &mut origin_planes {
        let visible = state.mode == ViewportMode::PickPlane;
        visibility.set_if_neq(if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if let Some(mut material) = plane_materials.get_mut(&handle.0) {
            let hovered = state.hovered_origin_plane == Some(plane.plane);
            let selected = state.selected_origin_plane == Some(plane.plane);
            let base_color = origin_plane_color(&palette.0, plane.plane);
            let base_color = rgba(
                base_color,
                if selected {
                    0.34
                } else if hovered {
                    0.28
                } else {
                    0.10
                },
            )
            .to_linear();
            if material.color != base_color {
                material.color = base_color;
            }
        }
    }
}

fn rebuild_native_face_overlays(
    mut commands: Commands,
    (model, presentation, section): (
        Res<ModelResource>,
        Res<PresentationResource>,
        Option<Res<section_view::State>>,
    ),
    palette: Res<PaletteResource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<
        (Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>),
        With<NativeCadFaceOverlay>,
    >,
    mut last: FaceOverlayCache,
) {
    if last
        .as_ref()
        .is_some_and(|stamp| stamp.matches(&model, &presentation.0, palette.0))
        && section.as_ref().is_none_or(|s| !s.is_changed())
    {
        return;
    }
    *last = Some(FaceOverlayStamp::capture(
        &model,
        &presentation.0,
        palette.0,
    ));

    for (entity, mesh, material) in &existing {
        meshes.remove(&mesh.0);
        materials.remove(&material.0);
        commands.entity(entity).despawn();
    }

    if section.as_ref().is_some_and(|s| s.active(&model)) {
        return;
    }

    let state = &presentation.0;
    let mut requested = state
        .selected_face_ids
        .iter()
        .copied()
        .map(|face_id| (face_id, true))
        .collect::<Vec<_>>();
    if let Some(face_id) = state.hovered_face_id {
        if !state.selected_face_ids.contains(&face_id) {
            requested.push((face_id, false));
        }
    }

    for (face_id, selected) in requested {
        let Some((body, face)) = model.document.scene.bodies.iter().find_map(|body| {
            body.faces
                .iter()
                .find(|face| face.id.0 == face_id)
                .map(|face| (body, face))
        }) else {
            continue;
        };
        if state.hidden_body_ids.contains(&body.id.0) {
            continue;
        }
        let Some(mesh) = face_mesh(body, face) else {
            continue;
        };
        let mesh_handle = meshes.add(mesh);
        let color = rgb(if selected {
            palette.0.face_selected
        } else {
            palette.0.face_hover
        });
        let material_handle = materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.32,
            metallic: 0.0,
            perceptual_roughness: 0.86,
            cull_mode: None,
            depth_bias: 1.0,
            ..default()
        });
        let occurrences = if model.instance_body_poses.is_empty() {
            vec![None]
        } else {
            model
                .instance_body_poses
                .iter()
                .filter(|instance| {
                    instance.body_id == body.id
                        && instance.visible
                        && if selected {
                            state.selected_occurrence_id.is_none()
                                || state.selected_occurrence_id == Some(instance.occurrence_id.0)
                        } else {
                            state.hovered_occurrence_id.is_none()
                                || state.hovered_occurrence_id == Some(instance.occurrence_id.0)
                        }
                })
                .map(|instance| Some(instance.occurrence_id.0))
                .collect::<Vec<_>>()
        };
        for occurrence_id in occurrences {
            commands.spawn((
                Name::new(format!(
                    "OCCT face highlight {face_id} occurrence {occurrence_id:?}"
                )),
                NativeCadFaceOverlay {
                    body_id: body.id.0,
                    occurrence_id,
                },
                NativeModelGeometry {
                    session_id: model.session_id.clone(),
                    geometry_revision: model.geometry_revision,
                    instance_revision: model.instance_revision,
                },
                Mesh3d(mesh_handle.clone()),
                MeshMaterial3d(material_handle.clone()),
                NotShadowCaster,
                NotShadowReceiver,
                instance_body_pose_transform(
                    &model.instance_body_poses,
                    &model.body_poses,
                    body.id.0,
                    occurrence_id,
                ),
            ));
        }
    }
}

fn apply_body_poses(model: Res<ModelResource>, mut entities: BodyPoseQuery) {
    if !model.is_changed() {
        return;
    }
    for (body, face, overlay, mut transform) in &mut entities {
        let identity = body
            .map(|body| (body.body_id, body.occurrence_id))
            .or_else(|| face.map(|face| (face.body_id, face.occurrence_id)))
            .or_else(|| overlay.map(|overlay| (overlay.body_id, overlay.occurrence_id)));
        if let Some((body_id, occurrence_id)) = identity {
            *transform = instance_body_pose_transform(
                &model.instance_body_poses,
                &model.body_poses,
                body_id,
                occurrence_id,
            );
        }
    }
}

/// Rebuilds renderer-neutral transient surfaces and manipulators. These meshes
/// stay separate from OCCT scene geometry: command fills may be translucent
/// overlays, while CAM remaining stock uses an opaque lit physical material.
/// Both remain replaceable at the simulator/editor's debounced update rate.
fn rebuild_native_preview_meshes(
    mut commands: Commands,
    preview: Res<PreviewResource>,
    mut revisions: ResMut<RenderedRevisions>,
    existing: Query<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>), With<NativePreviewMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if revisions.preview_meshes == preview.mesh_revision {
        return;
    }
    revisions.preview_meshes = preview.mesh_revision;

    for (entity, mesh, material) in &existing {
        meshes.remove(mesh.0.id());
        materials.remove(material.0.id());
        commands.entity(entity).despawn();
    }

    for layer in &preview.value.triangles {
        let positions = layer
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|point| {
                point
                    .iter()
                    .all(|value| value.is_finite())
                    .then_some([point[0], point[1], point[2]])
            })
            .collect::<Vec<_>>();
        if positions.len() < 3 || positions.len() % 3 != 0 {
            continue;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        let all_positions_valid = positions.len() * 3 == layer.positions.len();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        let normals = layer
            .normals
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|normal| {
                normal
                    .iter()
                    .all(|value| value.is_finite())
                    .then_some([normal[0], normal[1], normal[2]])
            })
            .collect::<Vec<_>>();
        if all_positions_valid
            && layer.normals.len() == layer.positions.len()
            && normals.len() * 3 == layer.normals.len()
        {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        } else {
            mesh.compute_flat_normals();
        }
        let color = Color::srgba(
            layer.color[0].clamp(0.0, 1.0),
            layer.color[1].clamp(0.0, 1.0),
            layer.color[2].clamp(0.0, 1.0),
            layer.color[3].clamp(0.0, 1.0),
        );
        let machined_stock =
            layer.material == crate::native_viewport::ViewportTriangleMaterial::MachinedStock;
        let mut entity = commands.spawn((
            Name::new(if machined_stock {
                "Native CAM remaining stock"
            } else {
                "Native command profile/tool fill"
            }),
            NativePreviewMesh,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: if machined_stock {
                    color.with_alpha(1.0)
                } else {
                    color
                },
                alpha_mode: if machined_stock {
                    AlphaMode::Opaque
                } else {
                    AlphaMode::Blend
                },
                unlit: !machined_stock,
                metallic: 0.0,
                perceptual_roughness: if machined_stock { 0.74 } else { 0.5 },
                double_sided: true,
                cull_mode: None,
                depth_bias: if machined_stock { 0.0 } else { 2.0 },
                ..default()
            })),
            NotShadowCaster,
        ));
        if !machined_stock {
            entity.insert(NotShadowReceiver);
        }
        if layer.xray {
            entity.insert(RenderLayers::layer(1));
        }
    }

    for arrow in &preview.value.arrows {
        let start = Vec3::from_array(arrow.start);
        let end = Vec3::from_array(arrow.end);
        if !start.is_finite() || !end.is_finite() {
            continue;
        }
        let delta = end - start;
        let length = delta.length();
        if !length.is_finite() || length <= 1.0e-5 {
            continue;
        }
        let color = Color::srgba(
            arrow.color[0].clamp(0.0, 1.0),
            arrow.color[1].clamp(0.0, 1.0),
            arrow.color[2].clamp(0.0, 1.0),
            arrow.color[3].clamp(0.0, 1.0),
        );
        let material = materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            depth_bias: 4.0,
            ..default()
        });
        let render_layer = arrow.xray.then(|| RenderLayers::layer(1));

        let mut shaft = commands.spawn((
            Name::new("Native Extrude direction shaft"),
            NativePreviewMesh,
            NativePreviewArrowPart {
                start,
                end,
                width: arrow.width,
                kind: NativePreviewArrowPartKind::Shaft,
            },
            Mesh3d(meshes.add(Cylinder::new(1.0, 1.0))),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        if let Some(layer) = render_layer.clone() {
            shaft.insert(layer);
        }

        let mut head = commands.spawn((
            Name::new("Native Extrude direction head"),
            NativePreviewMesh,
            NativePreviewArrowPart {
                start,
                end,
                width: arrow.width,
                kind: NativePreviewArrowPartKind::Head,
            },
            Mesh3d(meshes.add(Cone::new(1.0, 1.0))),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        if let Some(layer) = render_layer.clone() {
            head.insert(layer);
        }

        let mut base = commands.spawn((
            Name::new("Native Extrude direction origin"),
            NativePreviewMesh,
            NativePreviewArrowPart {
                start,
                end,
                width: arrow.width,
                kind: NativePreviewArrowPartKind::Base,
            },
            Mesh3d(meshes.add(Sphere::new(1.0))),
            MeshMaterial3d(material),
            Transform::default(),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        if let Some(layer) = render_layer {
            base.insert(layer);
        }
    }
}

/// Upload the desktop simulator's retained stock directly from Rust. This is
/// intentionally independent of `PreviewResource`: toolpath/highlight updates
/// cannot tear down or resend the large physical stock mesh.
#[derive(Default)]
struct CamDisplayMeshCache {
    entries: std::collections::VecDeque<(ViewportCamStock, Handle<Mesh>)>,
}

fn rebuild_native_cam_stock(
    mut commands: Commands,
    (stock, presentation): (Res<CamStockResource>, Res<PresentationResource>),
    mut revisions: ResMut<RenderedRevisions>,
    existing: Query<(Entity, &Mesh3d), With<NativeCamStockMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    gpu_stock: Res<GpuStock>,
    mut cache: Local<CamDisplayMeshCache>,
) {
    if revisions.cam_stock == stock.revision {
        return;
    }
    revisions.cam_stock = stock.revision;

    let Some(stock) = &stock.value else {
        for (entity, _mesh) in &existing {
            commands.entity(entity).despawn();
        }
        for (_, mesh) in cache.entries.drain(..) {
            meshes.remove(mesh.id());
        }
        return;
    };
    let cached = cache
        .entries
        .iter()
        .position(|(source, _)| {
            source.positions == stock.positions && source.normals == stock.normals
        })
        .and_then(|index| cache.entries.remove(index));
    let handle = if let Some(entry) = cached {
        let handle = entry.1.clone();
        cache.entries.push_back(entry);
        handle
    } else {
        let positions = stock
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|point| [point[0], point[1], point[2]])
            .collect::<Vec<_>>();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        if stock.normals.len() == stock.positions.len() {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_NORMAL,
                stock
                    .normals
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|normal| [normal[0], normal[1], normal[2]])
                    .collect::<Vec<_>>(),
            );
        } else {
            mesh.compute_flat_normals();
        }
        let handle = meshes.add(mesh);
        cache.entries.push_back((stock.clone(), handle.clone()));

        while cache.entries.len() > 4
            || (cache.entries.len() > 1
                && cache
                    .entries
                    .iter()
                    .map(|(source, _)| (source.positions.len() + source.normals.len()) * 4 * 3)
                    .sum::<usize>()
                    > 32 * 1024 * 1024)
        {
            if let Some((_, old)) = cache.entries.pop_front() {
                meshes.remove(old.id());
            }
        }
        handle
    };
    if let Some((entity, _)) = existing.iter().next() {
        commands.entity(entity).insert(Mesh3d(handle));
        return;
    }
    commands.spawn((
        Name::new("Native retained CAM remaining stock"),
        NativeCamStockMesh,
        Mesh3d(handle),
        MeshMaterial3d(gpu_stock.clip.clone()),
        if presentation.0.cam_stock_visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        },
        NotShadowCaster,
    ));
}

fn update_native_cam_stock_visibility(
    presentation: Res<PresentationResource>,
    mut stock: Query<&mut Visibility, With<NativeCamStockMesh>>,
) {
    if !presentation.is_changed() {
        return;
    }
    let visibility = if presentation.0.cam_stock_visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut current in &mut stock {
        *current = visibility;
    }
}

#[derive(Clone, Copy, PartialEq)]
struct GpuStockInputStamp {
    enabled: bool,
    stock_revision: u64,
    preview_revision: u64,
    cursor: Option<super::ViewportCamPathProgress>,
    tool: Option<ViewportCamTool>,
}

fn setup_gpu_stock(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut clip_materials: ResMut<Assets<super::gpu_stock::StockClipMaterial>>,
    mut cut_materials: ResMut<Assets<super::gpu_stock::CutSurfaceMaterial>>,
) {
    commands.insert_resource(GpuStock::new(
        &mut images,
        &mut clip_materials,
        &mut cut_materials,
        cam_stock_material(),
    ));
}

/// Advance GPU stock removal to the playback cursor. Runs every frame but
/// only re-extracts travel when the cursor, path or retained stock changed.
#[allow(clippy::too_many_arguments)]
fn update_gpu_stock(
    mut commands: Commands,
    presentation: Res<PresentationResource>,
    preview: Res<PreviewResource>,
    stock: Res<CamStockResource>,
    mut gpu_stock: ResMut<GpuStock>,
    mut stamp: ResMut<GpuStockStamp>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut clip_materials: ResMut<Assets<super::gpu_stock::StockClipMaterial>>,
    mut cut_materials: ResMut<Assets<super::gpu_stock::CutSurfaceMaterial>>,
    mut visibility: Query<&mut Visibility>,
    mut last: Local<Option<GpuStockInputStamp>>,
) {
    if !presentation.is_changed() && !preview.is_changed() && !stock.is_changed() {
        return;
    }
    let state = &presentation.0;
    let next = GpuStockInputStamp {
        enabled: state.cam_gpu_stock_removal && state.cam_stock_visible,
        stock_revision: stock.revision,
        preview_revision: preview.revision,
        cursor: state.cam_path_progress,
        tool: state.cam_tool,
    };
    if last.as_ref() == Some(&next) {
        return;
    }
    *last = Some(next);
    let value = stock.value.as_ref();
    let revision = stamp.revision();
    gpu_stock.update(
        GpuStockInputs {
            enabled: state.cam_gpu_stock_removal && state.cam_stock_visible,
            stock_positions: value.map(|stock| stock.positions.as_slice()),
            stock_time: value.and_then(|stock| stock.time_seconds),
            stock_revision: stock.revision,
            cursor: state.cam_path_progress,
            tool: state.cam_tool,
            lines: &preview.value.lines,
        },
        &mut commands,
        &mut images,
        &mut meshes,
        &mut clip_materials,
        &mut cut_materials,
        stamp.bypass_change_detection(),
        &mut visibility,
    );
    if stamp.revision() != revision {
        stamp.set_changed();
    }
}

/// The playback cutter is semantic retained Bevy geometry, not transient
/// triangle soup. A clock tick therefore moves two unit cylinders without
/// hashing, serializing, or reallocating the static stock and toolpath meshes.
fn update_native_cam_tool(
    mut commands: Commands,
    presentation: Res<PresentationResource>,
    mut existing: Query<(
        &mut NativeCamToolPart,
        &Mesh3d,
        &mut Transform,
        &mut Visibility,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !presentation.is_changed() {
        return;
    }
    let Some(tool) = presentation
        .0
        .cam_tool
        .filter(|_| !presentation.0.cam_tool_hidden)
    else {
        for (_, _, _, mut visibility) in &mut existing {
            *visibility = Visibility::Hidden;
        }
        return;
    };

    let needs_mesh = existing.iter().count() < 2
        || existing
            .iter()
            .any(|(part, _, _, _)| part.geometry != tool.geometry);
    let shape = if needs_mesh {
        match limo_cad_cam::cutter_mesh(tool.geometry) {
            Ok(shape) => Some(shape),
            Err(_) => {
                for (_, _, _, mut visibility) in &mut existing {
                    *visibility = Visibility::Hidden;
                }
                return;
            }
        }
    } else {
        None
    };
    let mut found_flute = false;
    let mut found_shank = false;
    for (mut part, handle, mut transform, mut visibility) in &mut existing {
        match part.kind {
            NativeCamToolPartKind::Flute => found_flute = true,
            NativeCamToolPartKind::Shank => found_shank = true,
        }
        if part.geometry != tool.geometry {
            if let Some(shape) = &shape {
                let source = match part.kind {
                    NativeCamToolPartKind::Flute => &shape.cutter,
                    NativeCamToolPartKind::Shank => &shape.shank,
                };
                if let Some(mut mesh) = meshes.get_mut(&handle.0) {
                    *mesh = cam_cutter_mesh(source);
                }
            }
            part.geometry = tool.geometry;
        }
        if let Some(next) = cam_tool_part_transform(tool, part.kind) {
            *transform = next;
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
    if found_flute && found_shank {
        return;
    }
    let Some(shape) = shape else {
        return;
    };
    for (kind, color, name, source) in [
        (
            NativeCamToolPartKind::Flute,
            Color::srgba(0.78, 0.80, 0.84, 0.85),
            "CAM cutter",
            &shape.cutter,
        ),
        (
            NativeCamToolPartKind::Shank,
            Color::srgba(0.62, 0.65, 0.70, 0.42),
            "CAM shank",
            &shape.shank,
        ),
    ] {
        if (kind == NativeCamToolPartKind::Flute && found_flute)
            || (kind == NativeCamToolPartKind::Shank && found_shank)
        {
            continue;
        }
        let transform = cam_tool_part_transform(tool, kind);
        commands.spawn((
            Name::new(name),
            NativeCamToolPart {
                kind,
                geometry: tool.geometry,
            },
            Mesh3d(meshes.add(cam_cutter_mesh(source))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                metallic: 0.65,
                perceptual_roughness: 0.32,
                cull_mode: None,
                ..default()
            })),
            transform.unwrap_or_default(),
            if transform.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

fn cam_cutter_mesh(source: &limo_cad_cam::CamCutterMeshPartDto) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        source
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        source
            .normals
            .as_chunks::<3>()
            .0
            .iter()
            .map(|n| [n[0], n[1], n[2]])
            .collect::<Vec<_>>(),
    );
    mesh
}

fn cam_tool_part_transform(
    tool: ViewportCamTool,
    kind: NativeCamToolPartKind,
) -> Option<Transform> {
    let tip = Vec3::from_array(tool.tip);
    let axis = Vec3::from_array(tool.axis).normalize_or_zero();
    if !tip.is_finite()
        || !axis.is_finite()
        || axis == Vec3::ZERO
        || (kind == NativeCamToolPartKind::Shank
            && tool.geometry.overall_length <= tool.geometry.flute_length)
    {
        return None;
    }

    Some(Transform::from_translation(tip).with_rotation(Quat::from_rotation_arc(Vec3::Z, axis)))
}

/// Preserve a constant logical-pixel arrow footprint without touching GPU
/// assets. This is intentionally cheap enough to run on every demanded frame.
fn update_native_preview_arrows(
    camera: Res<CameraResource>,
    viewport: Res<ViewportSizeResource>,
    mut arrows: Query<(&NativePreviewArrowPart, &mut Transform)>,
) {
    for (arrow, mut transform) in &mut arrows {
        let delta = arrow.end - arrow.start;
        let length = delta.length();
        if !length.is_finite() || length <= 1.0e-5 {
            *transform = Transform::from_scale(Vec3::ZERO);
            continue;
        }
        let direction = delta / length;
        let center = arrow.start + delta * 0.5;
        let world_per_pixel = world_per_pixel_at(camera.camera, *viewport, center);
        let width = arrow.width.clamp(1.0, 4.0);
        let shaft_radius = (world_per_pixel * width * 0.46).max(length * 0.003);
        let head_length = (world_per_pixel * 11.0)
            .max(length * 0.10)
            .min(length * 0.42);
        let shaft_length = (length - head_length).max(length * 0.05);
        let head_radius = (world_per_pixel * width * 2.2).max(shaft_radius * 2.5);
        let rotation = Quat::from_rotation_arc(Vec3::Y, direction);

        *transform = match arrow.kind {
            NativePreviewArrowPartKind::Shaft => {
                Transform::from_translation(arrow.start + direction * (shaft_length * 0.5))
                    .with_rotation(rotation)
                    .with_scale(Vec3::new(shaft_radius, shaft_length, shaft_radius))
            }
            NativePreviewArrowPartKind::Head => Transform::from_translation(
                arrow.start + direction * (shaft_length + head_length * 0.5),
            )
            .with_rotation(rotation)
            .with_scale(Vec3::new(head_radius, head_length, head_radius)),
            NativePreviewArrowPartKind::Base => {
                Transform::from_translation(arrow.start).with_scale(Vec3::splat(head_radius * 0.54))
            }
        };
    }
}

fn rebuild_native_annotations(
    mut commands: Commands,
    preview: Res<PreviewResource>,
    palette: Res<PaletteResource>,
    assets: Res<ViewportUiAssets>,
    mut revisions: ResMut<RenderedRevisions>,
    existing: Query<Entity, With<NativeAnnotationRoot>>,
    cameras: Query<Entity, With<NativeOverlayCamera>>,
) {
    if revisions.annotations == preview.revision && !palette.is_changed() {
        return;
    }
    revisions.annotations = preview.revision;

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let Ok(camera) = cameras.single() else {
        return;
    };

    for annotation in &preview.value.annotations {
        if (annotation.text.trim().is_empty()
            && annotation.icon.is_none()
            && annotation.tool_icon.is_none())
            || !annotation.screen[0].is_finite()
            || !annotation.screen[1].is_finite()
        {
            continue;
        }
        let constraint = annotation.kind == ViewportAnnotationKind::Constraint;
        let tool = annotation.kind == ViewportAnnotationKind::Tool;
        let selected = annotation.selected;

        let annotation_transform = UiTransform::from_translation(Val2::percent(-50.0, -50.0));
        let foreground = if selected {
            rgb(palette.0.ink)
        } else {
            Color::srgba(
                annotation.color[0],
                annotation.color[1],
                annotation.color[2],
                annotation.color[3].clamp(0.0, 1.0),
            )
        };
        let (min_width, min_height, pad_x, pad_y, radius, font_size, border) = if constraint {
            if selected {
                (28.0, 24.0, 2.0, 1.0, 5.0, 11.0, 2.0)
            } else {
                (22.0, 22.0, 0.0, 0.0, 0.0, 9.0, 0.0)
            }
        } else if tool {
            (24.0, 24.0, 3.0, 3.0, 5.0, 9.0, 1.0)
        } else if selected {
            (28.0, 20.0, 5.0, 2.0, 5.0, 11.0, 2.0)
        } else {
            (24.0, 18.0, 4.0, 2.0, 5.0, 10.0, 1.0)
        };
        commands
            .spawn((
                Name::new(format!("Native viewport annotation {}", annotation.text)),
                NativeAnnotationRoot,
                UiTargetCamera(camera),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(annotation.screen[0]),
                    top: px(annotation.screen[1]),
                    min_width: px(min_width),
                    min_height: px(min_height),
                    padding: UiRect::axes(px(pad_x), px(pad_y)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(border)),
                    border_radius: BorderRadius::all(px(radius)),
                    ..default()
                },
                annotation_transform,
                BackgroundColor(if selected {
                    rgba(palette.0.accent, 0.92)
                } else if tool {
                    rgba(palette.0.header, 0.78)
                } else if constraint {
                    Color::NONE
                } else {
                    rgba(palette.0.header, 0.90)
                }),
                BorderColor::all(rgba(
                    if selected {
                        palette.0.selection
                    } else {
                        palette.0.ui_edge
                    },
                    if border == 0.0 {
                        0.0
                    } else if selected {
                        1.0
                    } else {
                        0.82
                    },
                )),
                ZIndex(if tool { 22 } else { 18 }),
            ))
            .with_children(|root| {
                if constraint {
                    if let Some(icon) = annotation.icon {
                        ui::spawn_constraint_icon(
                            root,
                            icon,
                            foreground,
                            if selected { 20.0 } else { 18.0 },
                        );
                        return;
                    }
                } else if tool {
                    if let Some(icon) = annotation.icon {
                        ui::spawn_constraint_icon(root, icon, foreground, 17.0);
                        return;
                    }
                    if let Some(icon) = annotation.tool_icon {
                        ui::spawn_tool_icon(root, icon, foreground, 16.0);
                        return;
                    }
                }
                root.spawn((
                    Text::new(annotation.text.clone()),
                    ViewportUiTheme::from_palette(&palette.0).text(
                        &assets,
                        font_size,
                        FontWeight::NORMAL,
                    ),
                    TextColor(foreground),
                ));
            });
    }
}

fn rebuild_native_hud(
    mut commands: Commands,
    (hud, palette): (Res<HudResource>, Res<PaletteResource>),
    assets: Res<ViewportUiAssets>,
    native_locale: Option<Res<crate::native_viewport::localization::NativeLocale>>,
    mut revisions: ResMut<RenderedRevisions>,
    existing: Query<Entity, With<NativeHudRoot>>,
    cameras: Query<Entity, With<NativeOverlayCamera>>,
) {
    if revisions.hud == hud.revision {
        return;
    }
    revisions.hud = hud.revision;

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let Ok(camera) = cameras.single() else {
        return;
    };

    ui::spawn_viewport_hud(
        &mut commands,
        camera,
        &hud.hud,
        &palette.0,
        &assets,
        crate::native_viewport::localization::locale_of(native_locale.as_deref()),
    );
}

fn update_native_hud_orientation(
    camera: Res<CameraResource>,
    mut marks: Query<(&HudAxisMark, &mut Node)>,
    mut labels: Query<(&HudAxisLabel, &mut Node), Without<HudAxisMark>>,
) {
    ui::update_orientation_nodes(camera.camera, &mut marks, &mut labels);
}

fn stable_view_up(direction: Vec3, up_hint: Vec3) -> Vec3 {
    let forward = direction.normalize_or_zero();
    if !forward.is_finite() || forward.length_squared() < 1.0e-8 {
        return Vec3::Z;
    }

    let mut up = up_hint.normalize_or_zero();
    up -= forward * up.dot(forward);
    if !up.is_finite() || up.length_squared() < 1.0e-8 {
        forward.any_orthonormal_vector()
    } else {
        up.normalize()
    }
}

fn camera_transform(camera: ViewportCamera) -> Transform {
    let position = Vec3::from_array(camera.position);
    let mut target = Vec3::from_array(camera.target);
    let direction = target - position;
    if !direction.is_finite() || direction.length_squared() < 1.0e-8 {
        target = position + Vec3::NEG_Z;
    }
    let up = stable_view_up(target - position, Vec3::from_array(camera.up));
    Transform::from_translation(position).looking_at(target, up)
}

fn origin_plane_bases() -> [(&'static str, PlaneBasis, Color); 3] {
    [
        (
            "XY",
            PlaneBasis {
                origin: [0.0, 0.0, 0.0],
                u: [1.0, 0.0, 0.0],
                v: [0.0, 1.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
            Color::srgba(0.25, 0.60, 0.94, 0.055),
        ),
        (
            "XZ",
            PlaneBasis {
                origin: [0.0, 0.0, 0.0],
                u: [1.0, 0.0, 0.0],
                v: [0.0, 0.0, 1.0],
                normal: [0.0, -1.0, 0.0],
            },
            Color::srgba(0.31, 0.74, 0.47, 0.050),
        ),
        (
            "YZ",
            PlaneBasis {
                origin: [0.0, 0.0, 0.0],
                u: [0.0, 1.0, 0.0],
                v: [0.0, 0.0, 1.0],
                normal: [1.0, 0.0, 0.0],
            },
            Color::srgba(0.88, 0.36, 0.39, 0.050),
        ),
    ]
}

fn origin_plane_color(palette: &ViewportPalette, plane: ViewportOriginPlane) -> [f32; 3] {
    match plane {
        ViewportOriginPlane::Xy => palette.origin_plane_xy,
        ViewportOriginPlane::Xz => palette.origin_plane_xz,
        ViewportOriginPlane::Yz => palette.origin_plane_yz,
    }
}

fn reference_plane_mesh(basis: &PlaneBasis, half_size: f32) -> Mesh {
    let origin = basis_vector(basis.origin);
    let u = basis_vector(basis.u) * half_size;
    let v = basis_vector(basis.v) * half_size;
    let normal = basis_vector(basis.normal).normalize_or_zero();
    let positions = vec![
        (origin - u - v).to_array(),
        (origin + u - v).to_array(),
        (origin + u + v).to_array(),
        (origin - u + v).to_array(),
    ];
    let normals = vec![normal.to_array(); 4];

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

fn body_mesh(body: &BodyDto) -> Option<Mesh> {
    if body.mesh.positions.len() < 9
        || !body.mesh.positions.len().is_multiple_of(3)
        || body.mesh.normals.len() != body.mesh.positions.len()
        || body
            .mesh
            .indices
            .iter()
            .any(|index| (*index as usize) * 3 + 2 >= body.mesh.positions.len())
    {
        return None;
    }
    let positions = body
        .mesh
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .map(|value| [value[0], value[1], value[2]])
        .collect::<Vec<_>>();
    let normals = body
        .mesh
        .normals
        .as_chunks::<3>()
        .0
        .iter()
        .map(|value| [value[0], value[1], value[2]])
        .collect::<Vec<_>>();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(body.mesh.indices.clone()));
    Some(mesh)
}

fn body_appearance_color(model: &ModelResource, body_id: u64, fallback: [f32; 3]) -> Color {
    let Some(appearance) = model
        .document
        .body_appearances
        .iter()
        .find(|appearance| appearance.body_id.0 == body_id)
    else {
        return rgb(fallback);
    };
    Color::srgb(
        f32::from(appearance.color.r) / 255.0,
        f32::from(appearance.color.g) / 255.0,
        f32::from(appearance.color.b) / 255.0,
    )
}

fn body_pose_transform(poses: &[BodyPoseDto], body_id: u64) -> Transform {
    let Some(pose) = poses.iter().find(|pose| pose.body_id.0 == body_id) else {
        return Transform::IDENTITY;
    };
    let rotation = Quat::from_xyzw(
        pose.rotation[0] as f32,
        pose.rotation[1] as f32,
        pose.rotation[2] as f32,
        pose.rotation[3] as f32,
    )
    .normalize();
    Transform::from_translation(Vec3::new(
        pose.translation[0] as f32,
        pose.translation[1] as f32,
        pose.translation[2] as f32,
    ))
    .with_rotation(rotation)
}

fn instance_layout_key(instance: &InstanceBodyPoseDto) -> (u64, u64, u64, bool) {
    (
        instance.occurrence_id.0,
        instance.component_id.0,
        instance.body_id.0,
        instance.visible,
    )
}

fn instance_body_pose_transform(
    instances: &[InstanceBodyPoseDto],
    legacy: &[BodyPoseDto],
    body_id: u64,
    occurrence_id: Option<u64>,
) -> Transform {
    let pose = occurrence_id.and_then(|occurrence_id| {
        instances
            .iter()
            .find(|pose| pose.body_id.0 == body_id && pose.occurrence_id.0 == occurrence_id)
    });
    let Some(pose) = pose else {
        return body_pose_transform(legacy, body_id);
    };
    let rotation = Quat::from_xyzw(
        pose.rotation[0] as f32,
        pose.rotation[1] as f32,
        pose.rotation[2] as f32,
        pose.rotation[3] as f32,
    )
    .normalize();
    Transform::from_translation(Vec3::new(
        pose.translation[0] as f32,
        pose.translation[1] as f32,
        pose.translation[2] as f32,
    ))
    .with_rotation(rotation)
}

fn visible_body_occurrences(model: &ModelResource, body_id: u64) -> Vec<Option<u64>> {
    if model.instance_body_poses.is_empty() {
        return vec![None];
    }
    model
        .instance_body_poses
        .iter()
        .filter(|instance| instance.body_id.0 == body_id && instance.visible)
        .map(|instance| Some(instance.occurrence_id.0))
        .collect()
}

fn face_mesh(body: &BodyDto, face: &FaceDto) -> Option<Mesh> {
    let start = face.first_index as usize;
    let end = start
        .saturating_add(face.index_count as usize)
        .min(body.mesh.indices.len());
    let mut positions = Vec::with_capacity(end.saturating_sub(start));
    let mut normals = Vec::with_capacity(end.saturating_sub(start));
    for vertex in &body.mesh.indices[start..end] {
        let offset = *vertex as usize * 3;
        let position = body.mesh.positions.get(offset..offset + 3)?;
        positions.push([position[0], position[1], position[2]]);
        if let Some(normal) = body.mesh.normals.get(offset..offset + 3) {
            normals.push([normal[0], normal[1], normal[2]]);
        }
    }
    if positions.len() < 3 {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    if normals.len() == end.saturating_sub(start) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    } else {
        mesh.compute_flat_normals();
    }
    Some(mesh)
}

#[derive(bevy::ecs::system::SystemParam)]
struct CadGizmos<'w, 's> {
    model_lines: (Gizmos<'w, 's>, Gizmos<'w, 's, CadModelEdgeGizmos>),
    sketch_gizmos: Gizmos<'w, 's, CadSketchGizmos>,
    sketch_point_outlines: Gizmos<'w, 's, CadSketchPointOutlineGizmos>,
    sketch_points: Gizmos<'w, 's, CadSketchPointGizmos>,
    cam_paths: (
        Gizmos<'w, 's, CadHighlightGizmos>,
        Gizmos<'w, 's, CamCompletedPathGizmos>,
    ),
    pick_halo: Gizmos<'w, 's, CadPickFeedbackHaloGizmos>,
    pick_feedback: Gizmos<'w, 's, CadPickFeedbackGizmos>,
    direct_pick_feedback: Gizmos<'w, 's, CadDirectPickFeedbackGizmos>,
    profile_borders: Gizmos<'w, 's, CadProfileBorderGizmos>,
}

fn draw_cad_gizmos(
    CadGizmos {
        model_lines,
        mut sketch_gizmos,
        mut sketch_point_outlines,
        mut sketch_points,
        cam_paths,
        mut pick_halo,
        mut pick_feedback,
        mut direct_pick_feedback,
        mut profile_borders,
    }: CadGizmos,
    model: (Res<ModelResource>, Query<&ModelEdgeCache>),
    (camera, viewport): (Res<CameraResource>, Res<ViewportSizeResource>),
    preview: Res<PreviewResource>,
    palette: Res<PaletteResource>,
    (presentation, section): (Res<PresentationResource>, Option<Res<section_view::State>>),
    face_boundaries: Query<(&NativeCadFace, &NativeModelGeometry)>,
) {
    let (model, edge_caches) = model;
    if section.as_ref().is_some_and(|s| s.active(&model)) {
        return;
    }
    let edge_cache = model
        .cache_entity
        .and_then(|entity| edge_caches.get(entity).ok());
    let (mut gizmos, mut model_edges) = model_lines;
    let (mut highlights, mut cam_completed) = cam_paths;
    let state = &presentation.0;
    let fine = rgba(palette.0.grid_fine, 0.28);
    let major = rgba(palette.0.grid_major, 0.48);

    keep_gizmo_asset_resident(&mut highlights);
    keep_gizmo_asset_resident(&mut model_edges);
    keep_gizmo_asset_resident(&mut cam_completed);
    keep_gizmo_asset_resident(&mut pick_halo);
    keep_gizmo_asset_resident(&mut pick_feedback);
    keep_gizmo_asset_resident(&mut direct_pick_feedback);
    keep_gizmo_asset_resident(&mut profile_borders);

    if state.mode == ViewportMode::Sketch {
        if let Some(sketch) = model
            .document
            .active_sketch
            .as_ref()
            .filter(|_| !state.hide_sketch_grid)
        {
            let layout = grid_layout(camera.camera, *viewport, &sketch.basis);
            draw_grid_on_basis(
                &mut gizmos,
                &sketch.basis,
                layout,
                camera.camera,
                *viewport,
                fine,
                major,
            );
        }
    } else {
        let [(_, ground, _), ..] = origin_plane_bases();
        let layout = grid_layout(camera.camera, *viewport, &ground);
        draw_grid_on_basis(
            &mut gizmos,
            &ground,
            layout,
            camera.camera,
            *viewport,
            fine,
            major,
        );
    }

    if state.mode == ViewportMode::PickPlane {
        let origin_half_size = reference_plane_half_size(camera.camera, *viewport, Vec3::ZERO);
        for (name, basis, _) in origin_plane_bases() {
            let plane = match name {
                "XY" => ViewportOriginPlane::Xy,
                "XZ" => ViewportOriginPlane::Xz,
                _ => ViewportOriginPlane::Yz,
            };
            let selected = state.selected_origin_plane == Some(plane);
            let hovered = state.hovered_origin_plane == Some(plane);
            let base_color = origin_plane_color(&palette.0, plane);
            let alpha = if selected {
                0.98
            } else if hovered {
                0.92
            } else {
                0.42
            };

            draw_plane_outline(
                &mut highlights,
                &basis,
                origin_half_size,
                rgba(base_color, alpha),
            );
        }
        gizmos.sphere(
            Vec3::ZERO,
            origin_half_size * 0.0092,
            Color::srgba(0.94, 0.95, 0.98, 0.98),
        );
        let axis_length = origin_half_size * 0.28;
        gizmos.arrow(
            Vec3::ZERO,
            Vec3::X * axis_length,
            Color::srgba(0.88, 0.36, 0.39, 0.98),
        );
        gizmos.arrow(
            Vec3::ZERO,
            Vec3::Y * axis_length,
            Color::srgba(0.35, 0.68, 0.45, 0.98),
        );
        gizmos.arrow(
            Vec3::ZERO,
            Vec3::Z * axis_length,
            Color::srgba(0.26, 0.65, 0.91, 0.98),
        );
    } else if state.mode == ViewportMode::Sketch {
        if let Some(sketch) = &model.document.active_sketch {
            let origin = basis_vector(sketch.basis.origin);
            gizmos.sphere(origin, 0.38, rgba(palette.0.mute, 0.92));
        }
    }

    for plane in &model.document.datum_planes {
        if state.hidden_datum_plane_ids.contains(&plane.datum_id.0) {
            continue;
        }
        let hovered = state.hovered_datum_plane_id == Some(plane.datum_id.0);
        let selected = state.selected_datum_plane_id == Some(plane.datum_id.0);
        let origin = basis_vector(plane.basis.origin);
        let half_size = reference_plane_half_size(camera.camera, *viewport, origin);
        if selected || hovered {
            draw_plane_outline(
                &mut highlights,
                &plane.basis,
                half_size,
                rgb(if selected {
                    palette.0.selection
                } else {
                    palette.0.hover
                }),
            );
        } else {
            draw_plane_outline(
                &mut sketch_gizmos,
                &plane.basis,
                half_size,
                if state.mode == ViewportMode::PickPlane {
                    rgba(palette.0.active_sketch, 0.72)
                } else {
                    Color::srgba(0.88, 0.68, 0.32, 0.56)
                },
            );
        }
    }

    for body in &model.document.scene.bodies {
        if state.hidden_body_ids.contains(&body.id.0) {
            continue;
        }
        let Some(metadata) = edge_cache.and_then(|cache| cache.bodies.get(&body.id.0)) else {
            continue;
        };
        let local_bounds = metadata.local_bounds;
        for occurrence_id in visible_body_occurrences(&model, body.id.0) {
            let occurrence_is_selected = state
                .selected_occurrence_id
                .is_none_or(|selected| occurrence_id == Some(selected));
            let selected_body_index = (occurrence_is_selected
                && state.selected_face_ids.is_empty()
                && state.selected_edge_ids.is_empty())
            .then(|| {
                state
                    .selected_body_ids
                    .iter()
                    .position(|body_id| *body_id == body.id.0)
            })
            .flatten();
            let hovered_body = state.hovered_body_id == Some(body.id.0)
                && state.hovered_edge_id.is_none()
                && state.hovered_face_id.is_none()
                && state
                    .hovered_occurrence_id
                    .is_none_or(|hovered| occurrence_id == Some(hovered));
            let body_transform = instance_body_pose_transform(
                &model.instance_body_poses,
                &model.body_poses,
                body.id.0,
                occurrence_id,
            );

            let ghosted_body = state.ghosted_body_ids.contains(&body.id.0)
                || model
                    .document
                    .active_sketch
                    .as_ref()
                    .and_then(|sketch| sketch.edit_occurrence_id)
                    .is_some_and(|editing| occurrence_id != Some(editing.0));
            let draw_default_edges = occurrence_edges_are_visible(
                local_bounds,
                &body_transform,
                camera.camera,
                *viewport,
            ) || ghosted_body;
            let lift_ceiling = local_bounds.map_or(f32::INFINITY, |(_, radius)| {
                radius
                    * body_transform.scale.max_element().abs().max(1.0e-6)
                    * MODEL_EDGE_MAX_LIFT_BODY_FRACTION
            });

            if selected_body_index.is_some() || hovered_body {
                let color = if selected_body_index == Some(0) {
                    rgb(palette.0.face_selected)
                } else if selected_body_index.is_some() {
                    rgb(palette.0.edge_selected)
                } else {
                    rgb(palette.0.edge_hover)
                };
                for edge in &body.edges {
                    draw_edge_segments(
                        &mut pick_halo,
                        edge,
                        rgb(palette.0.pick_halo),
                        &body_transform,
                        None,
                    );
                    draw_edge_segments(&mut pick_feedback, edge, color, &body_transform, None);
                }
            }

            for (edge, sides) in body.edges.iter().zip(&metadata.sides) {
                let selected =
                    occurrence_is_selected && state.selected_edge_ids.contains(&edge.id.0);
                let hovered = state.hovered_edge_id == Some(edge.id.0)
                    && state
                        .hovered_occurrence_id
                        .is_none_or(|hovered| occurrence_id == Some(hovered));
                let eligible = (state.pick_refinable_edges && edge.refinable)
                    || (state.pick_straight_edges && edge_is_straight(edge));
                if !draw_default_edges
                    && !selected
                    && !hovered
                    && !eligible
                    && selected_body_index.is_none()
                    && !hovered_body
                {
                    continue;
                }
                let color = if selected {
                    palette.0.edge_selected
                } else if hovered {
                    palette.0.edge_hover
                } else if eligible {
                    palette.0.finished_sketch
                } else if selected_body_index.is_some() {
                    palette.0.body_selected_edge
                } else {
                    palette.0.edge
                };
                if ghosted_body && !selected && !hovered && selected_body_index.is_none() {
                    draw_edge_segments(&mut highlights, edge, rgb(color), &body_transform, None);
                } else {
                    draw_edge_segments(
                        &mut model_edges,
                        edge,
                        rgba(color, 0.92),
                        &body_transform,
                        Some(EdgeLift {
                            camera: camera.camera,
                            viewport: *viewport,
                            sides: transform_edge_sides(*sides, &body_transform),
                            ceiling: lift_ceiling,
                        }),
                    );
                }
                if selected || hovered {
                    draw_edge_segments(
                        &mut pick_halo,
                        edge,
                        rgb(palette.0.pick_halo),
                        &body_transform,
                        None,
                    );
                    draw_edge_segments(
                        &mut pick_feedback,
                        edge,
                        rgb(if selected {
                            palette.0.edge_selected
                        } else {
                            palette.0.edge_hover
                        }),
                        &body_transform,
                        None,
                    );
                }
            }
        }
    }

    for (face, geometry) in &face_boundaries {
        if geometry.session_id != model.session_id || state.hidden_body_ids.contains(&face.body_id)
        {
            continue;
        }
        let occurrence_is_selected = state
            .selected_occurrence_id
            .is_none_or(|selected| face.occurrence_id == Some(selected));
        let selected = occurrence_is_selected && state.selected_face_ids.contains(&face.face_id);
        let hovered = state.hovered_face_id == Some(face.face_id)
            && state
                .hovered_occurrence_id
                .is_none_or(|hovered| face.occurrence_id == Some(hovered));
        if !selected && !hovered {
            continue;
        }
        let color = rgb(if selected {
            palette.0.edge_selected
        } else {
            palette.0.edge_hover
        });
        let transform = instance_body_pose_transform(
            &model.instance_body_poses,
            &model.body_poses,
            face.body_id,
            face.occurrence_id,
        );
        for (start, end) in face.boundary.iter() {
            pick_halo.line(
                transform.transform_point(*start),
                transform.transform_point(*end),
                rgb(palette.0.pick_halo),
            );
            pick_feedback.line(
                transform.transform_point(*start),
                transform.transform_point(*end),
                color,
            );
        }
    }

    for sketch in &model.document.finished_sketches {
        if state.hidden_sketch_names.contains(&sketch.name) {
            continue;
        }
        let curve_color = rgba(palette.0.finished_sketch, 0.58);
        let profile_loops = model
            .document
            .profile_catalog
            .iter()
            .filter(|catalog| state.profile_picker_active && catalog.sketch_name == sketch.name)
            .flat_map(|catalog| {
                catalog.profiles.iter().filter(|profile| {
                    let outer_index = if profile.nesting_depth % 2 == 0 {
                        Some(profile.index)
                    } else {
                        profile.parent_index
                    };
                    state.candidate_profiles.iter().any(|reference| {
                        reference.sketch_name == catalog.sketch_name
                            && Some(reference.profile_index) == outer_index
                    })
                })
            })
            .collect::<Vec<_>>();
        let point_color = rgb(palette.0.finished_sketch_point);
        let point_outline_color = rgba(palette.0.finished_sketch_point_outline, 0.96);
        for entity in &sketch.entities {
            let (entity_id, _) = sketch_entity_style(entity);
            let direct_feedback = state
                .selected_finished_sketch_entities
                .iter()
                .any(|candidate| {
                    candidate.sketch_name == sketch.name && candidate.entity_id == entity_id
                })
                || state
                    .hovered_finished_sketch_entity
                    .as_ref()
                    .is_some_and(|candidate| {
                        candidate.sketch_name == sketch.name && candidate.entity_id == entity_id
                    });

            if !direct_feedback {
                draw_base_curve_outside_profiles(
                    &mut sketch_gizmos,
                    &sketch.basis,
                    entity,
                    curve_color,
                    &profile_loops,
                );
            }
            draw_sketch_entity_grips(
                &mut sketch_point_outlines,
                &sketch.basis,
                entity,
                (camera.camera, *viewport),
                SKETCH_POINT_OUTLINE_RADIUS_PX,
                point_outline_color,
                false,
            );
            draw_sketch_entity_grips(
                &mut sketch_points,
                &sketch.basis,
                entity,
                (camera.camera, *viewport),
                SKETCH_POINT_RADIUS_PX,
                point_color,
                false,
            );
        }
    }

    if state.profile_picker_active {
        for catalog in &model.document.profile_catalog {
            if state.hidden_sketch_names.contains(&catalog.sketch_name) {
                continue;
            }

            let replacement_lines = model
                .document
                .finished_sketches
                .iter()
                .filter(|sketch| sketch.name == catalog.sketch_name)
                .flat_map(|sketch| &sketch.entities)
                .filter_map(|entity| {
                    let EntityDto::Line { id, start, end, .. } = entity else {
                        return None;
                    };
                    let directly_highlighted = state
                        .selected_finished_sketch_entities
                        .iter()
                        .chain(state.hovered_finished_sketch_entity.iter())
                        .any(|reference| {
                            reference.sketch_name == catalog.sketch_name
                                && reference.entity_id == id.0
                        });
                    directly_highlighted.then_some([
                        Point2Dto::new(start.x, start.y),
                        Point2Dto::new(end.x, end.y),
                    ])
                })
                .collect::<Vec<_>>();
            for profile in catalog
                .profiles
                .iter()
                .filter(|candidate| candidate.nesting_depth % 2 == 0)
            {
                if !state.candidate_profiles.iter().any(|candidate| {
                    candidate.sketch_name == catalog.sketch_name
                        && candidate.profile_index == profile.index
                }) {
                    continue;
                }
                let selected = state.selected_profiles.iter().any(|candidate| {
                    candidate.sketch_name == catalog.sketch_name
                        && candidate.profile_index == profile.index
                });
                let hovered = state.hovered_profile.as_ref().is_some_and(|candidate| {
                    candidate.sketch_name == catalog.sketch_name
                        && candidate.profile_index == profile.index
                });
                let color = if selected {
                    rgb(palette.0.edge_selected)
                } else if hovered {
                    rgb(palette.0.edge_hover)
                } else {
                    rgba(palette.0.finished_sketch, 0.94)
                };
                let mut outline = profile_outline_segments(&profile.points, &replacement_lines);
                for hole in catalog.profiles.iter().filter(|candidate| {
                    candidate.nesting_depth % 2 == 1
                        && candidate.parent_index == Some(profile.index)
                }) {
                    outline.extend(profile_outline_segments(&hole.points, &replacement_lines));
                }
                draw_profile_segments(&mut profile_borders, &catalog.basis, &outline, color);
            }
        }
    }

    for sketch in &model.document.finished_sketches {
        if state.hidden_sketch_names.contains(&sketch.name) {
            continue;
        }
        for entity in &sketch.entities {
            let (entity_id, _) = sketch_entity_style(entity);
            let selected = state
                .selected_finished_sketch_entities
                .iter()
                .any(|candidate| {
                    candidate.sketch_name == sketch.name && candidate.entity_id == entity_id
                });
            let hovered = state
                .hovered_finished_sketch_entity
                .as_ref()
                .is_some_and(|candidate| {
                    candidate.sketch_name == sketch.name && candidate.entity_id == entity_id
                });
            if selected {
                draw_sketch_curve_at_offset(
                    &mut direct_pick_feedback,
                    &sketch.basis,
                    entity,
                    rgb(palette.0.accent),
                    DIRECT_PICK_FEEDBACK_OFFSET,
                );
            } else if hovered {
                draw_sketch_curve_at_offset(
                    &mut direct_pick_feedback,
                    &sketch.basis,
                    entity,
                    rgb(palette.0.hover),
                    DIRECT_PICK_FEEDBACK_OFFSET,
                );
            }

            for reference in state.selected_sketch_points.iter().filter(|reference| {
                reference.sketch_name == sketch.name && reference.entity_id == entity_id
            }) {
                if let Some(position) = referenced_sketch_point(entity, reference) {
                    let point = hole_point_marker_world(
                        &sketch.basis,
                        position,
                        state.sketch_point_support_plane.as_ref(),
                    );
                    draw_screen_dot(
                        &mut pick_halo,
                        point,
                        camera.camera,
                        *viewport,
                        6.0,
                        rgb(palette.0.pick_halo),
                    );
                    draw_screen_dot(
                        &mut pick_feedback,
                        point,
                        camera.camera,
                        *viewport,
                        6.0,
                        rgb(palette.0.edge_selected),
                    );
                }
            }
            if let Some(reference) = state.hovered_sketch_point.as_ref().filter(|reference| {
                reference.sketch_name == sketch.name
                    && reference.entity_id == entity_id
                    && !state.selected_sketch_points.contains(reference)
            }) {
                if let Some(position) = referenced_sketch_point(entity, reference) {
                    let point = hole_point_marker_world(
                        &sketch.basis,
                        position,
                        state.sketch_point_support_plane.as_ref(),
                    );
                    draw_screen_dot(
                        &mut pick_halo,
                        point,
                        camera.camera,
                        *viewport,
                        7.0,
                        rgb(palette.0.pick_halo),
                    );
                    draw_screen_dot(
                        &mut pick_feedback,
                        point,
                        camera.camera,
                        *viewport,
                        7.0,
                        rgb(palette.0.edge_hover),
                    );
                }
            }
        }
    }

    let (camera_right, camera_up) = camera_facing_axes(camera.camera);
    if let Some(point) = &state.hovered_surface_point {
        let world = Vec3::new(point.x as f32, point.y as f32, point.z as f32);
        let radius = screen_space_disc_radius(camera.camera, *viewport, world, 6.0);
        draw_filled_disc(
            &mut pick_halo,
            world,
            camera_right,
            camera_up,
            radius,
            rgb(palette.0.pick_halo),
            6,
        );
        draw_filled_disc(
            &mut pick_feedback,
            world,
            camera_right,
            camera_up,
            radius,
            rgb(palette.0.edge_hover),
            6,
        );
    }
    if let Some(point) = &state.selected_surface_point {
        let world = Vec3::new(point.x as f32, point.y as f32, point.z as f32);
        let radius = screen_space_disc_radius(camera.camera, *viewport, world, 7.0);
        draw_filled_disc(
            &mut pick_halo,
            world,
            camera_right,
            camera_up,
            radius,
            rgb(palette.0.pick_halo),
            7,
        );
        draw_filled_disc(
            &mut pick_feedback,
            world,
            camera_right,
            camera_up,
            radius,
            rgb(palette.0.edge_selected),
            7,
        );
    }

    if let Some(sketch) = &model.document.active_sketch {
        if !state.hide_projected_geometry {
            draw_projected_edges(&mut sketch_gizmos, sketch, rgb(palette.0.projected));
        }
        draw_sketch(
            &mut sketch_gizmos,
            sketch,
            camera.camera,
            *viewport,
            3.5,
            |entity| {
                let (_, fully_defined) = sketch_entity_style(entity);
                Some(rgb(if fully_defined {
                    palette.0.defined_sketch
                } else {
                    palette.0.active_sketch
                }))
            },
            !state.hide_sketch_points,
        );
        draw_sketch(
            &mut pick_feedback,
            sketch,
            camera.camera,
            *viewport,
            5.0,
            |entity| {
                let (id, _) = sketch_entity_style(entity);
                if state.hovered_sketch_entity_id == Some(id) {
                    Some(rgb(palette.0.hover))
                } else {
                    None
                }
            },
            true,
        );
        draw_sketch(
            &mut highlights,
            sketch,
            camera.camera,
            *viewport,
            4.5,
            |entity| {
                let (id, _) = sketch_entity_style(entity);
                if state.selected_sketch_entity_ids.contains(&id) {
                    Some(rgb(palette.0.selection))
                } else if state.constraint_related_sketch_entity_ids.contains(&id) {
                    Some(rgb(palette.0.constraint_related))
                } else {
                    None
                }
            },
            true,
        );
    }

    for completed_pass in [false, true] {
        for layer in preview
            .value
            .lines
            .iter()
            .chain(&preview.sketch_lines)
            .filter(|layer| !layer.hidden)
        {
            let layer_color = layer.color_role.resolve(layer.color, &palette.0);
            let playback = layer.playback.as_ref();

            let cursor = active_cursor(playback, state.cam_path_progress);
            if completed_pass != playback.is_some() || (playback.is_some() && cursor.is_none()) {
                continue;
            }
            let completed_color = playback.map_or(layer_color, |path| path.completed_color);
            for (index, segment) in layer.segments.as_chunks::<6>().0.iter().enumerate() {
                let start = Vec3::new(segment[0], segment[1], segment[2]);
                let end = Vec3::new(segment[3], segment[4], segment[5]);
                let timing = playback
                    .and_then(|path| path.segment_times.get(index * 2..index * 2 + 2))
                    .map(|pair| [pair[0], pair[1]]);
                if let (Some([begin, finish]), Some(cursor)) = (timing, cursor) {
                    if (completed_pass
                        && cursor.time_seconds <= begin + 1e-9
                        && cursor.time_seconds + 1e-9 < finish)
                        || (!completed_pass && cursor.time_seconds + 1e-9 >= finish)
                    {
                        continue;
                    }
                }
                let mut draw = |start, end, timing| {
                    for part in
                        split_segment(start, end, layer_color, completed_color, timing, cursor)
                            .into_iter()
                            .flatten()
                    {
                        if part.completed != completed_pass {
                            continue;
                        }
                        let color = Color::srgba(
                            part.color[0],
                            part.color[1],
                            part.color[2],
                            part.color[3].clamp(0.0, 1.0),
                        );
                        if part.completed {
                            cam_completed.line(part.start, part.end, color);
                        } else if layer.width >= 2.0 {
                            highlights.line(part.start, part.end, color);
                        } else {
                            gizmos.line(part.start, part.end, color);
                        }
                    }
                };
                if layer.pattern == ViewportLinePattern::Dotted {
                    let delta = end - start;
                    let length = delta.length();
                    if length <= f32::EPSILON {
                        continue;
                    }
                    let world_per_pixel =
                        world_per_pixel_at(camera.camera, *viewport, start.lerp(end, 0.5))
                            .max(f32::EPSILON);

                    let dot_length = world_per_pixel * 1.25;
                    let requested_period = world_per_pixel * 4.25;
                    let direction = delta / length;
                    let requested_count = ((length / requested_period).ceil() as usize).max(1);
                    let dot_count = requested_count.min(512);
                    let period = if requested_count > dot_count {
                        length / dot_count as f32
                    } else {
                        requested_period
                    };
                    for dot_index in 0..dot_count {
                        let distance = dot_index as f32 * period;
                        if distance >= length {
                            break;
                        }
                        let dot_start = start + direction * distance;
                        let end_distance = (distance + dot_length).min(length);
                        let dot_end = start + direction * end_distance;

                        let dot_timing = timing.map(|[begin, finish]| {
                            let duration = finish - begin;
                            [
                                begin + duration * f64::from(distance / length),
                                begin + duration * f64::from(end_distance / length),
                            ]
                        });
                        draw(dot_start, dot_end, dot_timing);
                    }
                } else {
                    draw(start, end, timing);
                }
            }
        }
    }

    for layer in &preview.value.points {
        let layer_color = layer.color_role.resolve(layer.color, &palette.0);
        let color = Color::srgba(
            layer_color[0],
            layer_color[1],
            layer_color[2],
            layer_color[3].clamp(0.0, 1.0),
        );
        let radius = layer.radius.clamp(0.08, 4.0);
        for point in layer.positions.as_chunks::<3>().0 {
            let center = Vec3::new(point[0], point[1], point[2]);
            let forward = (Vec3::from_array(camera.camera.target)
                - Vec3::from_array(camera.camera.position))
            .normalize_or_zero();
            let up_hint = Vec3::from_array(camera.camera.up).normalize_or_zero();
            let right = forward.cross(up_hint).normalize_or_zero();
            let right = if right == Vec3::ZERO { Vec3::X } else { right };
            let up = right.cross(forward).normalize_or_zero();
            if layer.hollow {
                let ring = (0..24)
                    .map(|index| {
                        let angle = index as f32 / 24.0 * std::f32::consts::TAU;
                        center + right * (angle.cos() * radius) + up * (angle.sin() * radius)
                    })
                    .collect::<Vec<_>>();
                draw_marker_loop(&mut highlights, &ring, color);
            } else {
                let world_per_pixel = world_per_pixel_at(camera.camera, *viewport, center);
                let half_steps = ((radius / world_per_pixel.max(0.001)).ceil() as i32).clamp(4, 96);
                draw_filled_disc(
                    &mut highlights,
                    center,
                    right,
                    up,
                    radius,
                    color,
                    half_steps,
                );
            }
        }
    }

    if let Some(marker) = preview.value.marker {
        draw_snap_marker(
            &mut highlights,
            marker,
            camera.camera,
            *viewport,
            &palette.0,
        );
    }
}

fn keep_gizmo_asset_resident<Config: GizmoConfigGroup>(gizmos: &mut Gizmos<Config>) {
    gizmos.line(Vec3::ZERO, Vec3::ZERO, Color::NONE);
}

fn draw_snap_marker(
    gizmos: &mut Gizmos<CadHighlightGizmos>,
    marker: ViewportSnapMarker,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    palette: &ViewportPalette,
) {
    let center = Vec3::from_array(marker.position);
    let camera_position = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - camera_position).normalize_or_zero();
    let camera_up = Vec3::from_array(camera.up).normalize_or_zero();
    let mut right = forward.cross(camera_up).normalize_or_zero();
    if right.length_squared() < 1.0e-8 {
        right = Vec3::X;
    }
    let mut up = right.cross(forward).normalize_or_zero();
    if up.length_squared() < 1.0e-8 {
        up = Vec3::Y;
    }
    let world_per_pixel = world_per_pixel_at(camera, viewport, center);
    let half = world_per_pixel * SNAP_MARKER_HALF_SIZE_PX;
    let point_color = rgba(palette.hover, 1.0);
    let secondary_color = rgba(palette.selection, 1.0);
    let preview_color = rgba(palette.preview, 0.98);

    match marker.kind {
        ViewportSnapKind::Point => {
            draw_marker_loop(
                gizmos,
                &[
                    center - right * half - up * half,
                    center + right * half - up * half,
                    center + right * half + up * half,
                    center - right * half + up * half,
                ],
                point_color,
            );
        }
        ViewportSnapKind::Midpoint | ViewportSnapKind::ReferenceMidpoint => {
            draw_marker_loop(
                gizmos,
                &[
                    center + up * half,
                    center + right * half - up * half,
                    center - right * half - up * half,
                ],
                secondary_color,
            );
        }
        ViewportSnapKind::Origin => {
            draw_marker_loop(
                gizmos,
                &[
                    center + up * half,
                    center + right * half,
                    center - up * half,
                    center - right * half,
                ],
                secondary_color,
            );
            let inner = half * 0.42;
            gizmos.line(
                center - right * inner,
                center + right * inner,
                secondary_color,
            );
            gizmos.line(center - up * inner, center + up * inner, secondary_color);
        }
        ViewportSnapKind::Curve => {
            draw_marker_loop(
                gizmos,
                &[
                    center + up * half,
                    center + right * half,
                    center - up * half,
                    center - right * half,
                ],
                point_color,
            );
        }
        ViewportSnapKind::Grid => {
            let arm = half * 0.72;
            gizmos.line(center - right * arm, center + right * arm, preview_color);
            gizmos.line(center - up * arm, center + up * arm, preview_color);
        }
    }
}

fn draw_marker_loop(gizmos: &mut Gizmos<CadHighlightGizmos>, points: &[Vec3], color: Color) {
    if points.len() < 2 {
        return;
    }
    for index in 0..points.len() {
        gizmos.line(points[index], points[(index + 1) % points.len()], color);
    }
}

/// Screen spacing the engine's sketch snap interval is chosen for. Shared with
/// the browser sketch grid (`TARGET_SKETCH_GRID_PX`) so the snap step is always
/// one of the fully drawn lattices below.
#[cfg(test)]
const GRID_TARGET_PX: f32 = 24.0;
/// Finest and coarsest intervals, in model millimetres.
const GRID_MIN_STEP: f32 = 0.001;
const GRID_MAX_STEP: f32 = 1_000_000.0;
/// A line fades in as the lattice it belongs to spreads from the first to the
/// second spacing on screen, and reads as a major line from the third to the
/// fourth. Brightness therefore follows the zoom continuously: nothing pops
/// when the finest drawn interval moves along the 1-2-5 sequence.
const GRID_LINE_FADE_IN_PX: [f32; 2] = [6.0, 20.0];
const GRID_MAJOR_FADE_PX: [f32; 2] = [120.0, 300.0];
/// Radius of the drawn sheet around the view centre, in viewport heights at
/// the target depth, so its edge fade stays put on screen while zooming.
const GRID_SHEET_RADIUS_HEIGHTS: f32 = 2.5;
/// Hard cap on lines drawn on each side of the centre per axis.
const GRID_MAX_HALF_LINES: i64 = 600;
/// How far below its plane the sheet sits, as a fraction of the finest drawn
/// interval, so it neither fights faces on the plane nor floats visibly under
/// sketch geometry when zoomed far in.
const GRID_PLANE_OFFSET_CELLS: f32 = 0.006;

#[cfg(test)]
fn one_two_five_mantissa(normalized: f64) -> f64 {
    if normalized < 2f64.sqrt() {
        1.0
    } else if normalized < 10f64.sqrt() {
        2.0
    } else if normalized < 50f64.sqrt() {
        5.0
    } else {
        10.0
    }
}

/// Nearest member of the 1-2-5 engineering sequence to the interval that
/// covers `GRID_TARGET_PX` at the view center, like `adaptiveSketchGridStep`
/// in the browser viewport.
#[cfg(test)]
fn adaptive_grid_step(world_per_pixel: f32) -> f32 {
    if !world_per_pixel.is_finite() || world_per_pixel <= 0.0 {
        return 10.0;
    }
    let desired =
        (f64::from(world_per_pixel) * f64::from(GRID_TARGET_PX)).max(f64::from(GRID_MIN_STEP));
    let decade = 10f64.powi(desired.log10().floor() as i32);
    ((one_two_five_mantissa(desired / decade) * decade) as f32).clamp(GRID_MIN_STEP, GRID_MAX_STEP)
}

/// Smallest member of the 1-2-5 sequence that is at least `value`.
fn one_two_five_ceiling(value: f32) -> f32 {
    if !value.is_finite() || value <= 0.0 {
        return GRID_MIN_STEP;
    }
    let value = f64::from(value.max(GRID_MIN_STEP));
    let decade = 10f64.powi(value.log10().floor() as i32);
    let normalized = value / decade;
    let mantissa = if normalized <= 1.0 + 1.0e-9 {
        1.0
    } else if normalized <= 2.0 + 1.0e-9 {
        2.0
    } else if normalized <= 5.0 + 1.0e-9 {
        5.0
    } else {
        10.0
    };
    ((mantissa * decade) as f32).clamp(GRID_MIN_STEP, GRID_MAX_STEP)
}

/// The coarsest 1-2-5 lattice a line belongs to, as a multiple of the finest
/// drawn interval, from the line's index in that finest lattice. The origin
/// line belongs to every lattice.
fn coarsest_lattice_ratio(index: i64, finest_mantissa: u8) -> f64 {
    if index == 0 {
        return f64::INFINITY;
    }

    let finest = 10 * i64::from(finest_mantissa);
    let coordinate = index.unsigned_abs().saturating_mul(finest as u64);
    let mut best = finest as u64;
    let mut decade: u64 = 1;
    while decade <= coordinate {
        for mantissa in [1u64, 2, 5] {
            let Some(step) = mantissa.checked_mul(decade) else {
                break;
            };
            if step >= finest as u64 && step > best && coordinate.is_multiple_of(step) {
                best = step;
            }
        }
        let Some(next) = decade.checked_mul(10) else {
            break;
        };
        decade = next;
    }
    best as f64 / finest as f64
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Colour of a grid line whose coarsest lattice spreads `pixel_spacing` on
/// screen: faint as it appears, minor while comfortably visible, major once
/// its lattice is wide enough to structure the sheet.
fn grid_line_color(pixel_spacing: f32, fine: Color, major: Color) -> Color {
    let presence = smoothstep(
        GRID_LINE_FADE_IN_PX[0],
        GRID_LINE_FADE_IN_PX[1],
        pixel_spacing,
    );
    let weight = smoothstep(GRID_MAJOR_FADE_PX[0], GRID_MAJOR_FADE_PX[1], pixel_spacing);
    let (fine, major) = (fine.to_srgba(), major.to_srgba());
    let mix = |a: f32, b: f32| a + (b - a) * weight;
    Color::srgba(
        mix(fine.red, major.red),
        mix(fine.green, major.green),
        mix(fine.blue, major.blue),
        mix(fine.alpha, major.alpha) * presence,
    )
}

/// Where and how densely the grid sheet is drawn for the current view.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GridLayout {
    /// Finest lattice interval drawn, a 1-2-5 member; its lines are the
    /// faintest and every coarser lattice is a subset of some finer one.
    finest: f32,
    /// Mantissa of `finest`: 1, 2 or 5.
    finest_mantissa: u8,
    /// Plane coordinates of the camera target: the sheet's fade is centred
    /// here and moves continuously with the view.
    center: Vec2,
    /// Sheet radius in model units, a fixed number of screen heights.
    radius: f32,
}

fn grid_layout(
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    basis: &PlaneBasis,
) -> GridLayout {
    let target = Vec3::from_array(camera.target);
    let world_per_pixel = world_per_pixel_at(camera, viewport, target);
    let finest = one_two_five_ceiling(world_per_pixel * GRID_LINE_FADE_IN_PX[0]);
    let decade = 10f64.powi(f64::from(finest).log10().floor() as i32);
    let finest_mantissa = (f64::from(finest) / decade).round() as u8;
    let local = target - basis_vector(basis.origin);
    GridLayout {
        finest,
        finest_mantissa: if [1, 2, 5].contains(&finest_mantissa) {
            finest_mantissa
        } else {
            1
        },
        center: Vec2::new(
            local.dot(basis_vector(basis.u)),
            local.dot(basis_vector(basis.v)),
        ),
        radius: world_per_pixel * viewport.logical_height.max(1.0) * GRID_SHEET_RADIUS_HEIGHTS,
    }
}

fn draw_grid_on_basis(
    gizmos: &mut Gizmos,
    basis: &PlaneBasis,
    layout: GridLayout,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    fine: Color,
    major: Color,
) {
    let GridLayout {
        finest,
        finest_mantissa,
        center,
        radius,
    } = layout;
    let origin = basis_vector(basis.origin)
        - basis_vector(basis.normal) * (finest * GRID_PLANE_OFFSET_CELLS);
    let u = basis_vector(basis.u);
    let v = basis_vector(basis.v);

    let fade = |offset: f32| (1.0 - (offset / radius).powi(2)).clamp(0.0, 1.0);
    let mut faded_line = |start: Vec3, end: Vec3, color: Color, weight: f32| {
        let middle = (start + end) * 0.5;
        let strong = color.with_alpha(color.alpha() * weight);
        let clear = color.with_alpha(0.0);
        gizmos.line_gradient(middle, start, strong, clear);
        gizmos.line_gradient(middle, end, strong, clear);
    };

    let mut lattice_line = |index: i64, middle: Vec3, start: Vec3, end: Vec3, lateral: f32| {
        let ratio = coarsest_lattice_ratio(index, finest_mantissa);
        let spacing = if ratio.is_finite() {
            (ratio as f32) * finest / world_per_pixel_at(camera, viewport, middle)
        } else {
            f32::INFINITY
        };
        let color = grid_line_color(spacing, fine, major);
        if color.alpha() > 0.002 {
            faded_line(start, end, color, fade(lateral));
        }
    };
    let range = |coordinate: f32| {
        let centre_index = (coordinate / finest).round() as i64;
        let span = ((radius / finest).ceil() as i64).min(GRID_MAX_HALF_LINES);
        (centre_index - span)..=(centre_index + span)
    };
    for index in range(center.x) {
        let coordinate = index as f32 * finest;
        let along_v = origin + u * coordinate;
        lattice_line(
            index,
            along_v + v * center.y,
            along_v + v * (center.y - radius),
            along_v + v * (center.y + radius),
            coordinate - center.x,
        );
    }
    for index in range(center.y) {
        let coordinate = index as f32 * finest;
        let along_u = origin + v * coordinate;
        lattice_line(
            index,
            along_u + u * center.x,
            along_u + u * (center.x - radius),
            along_u + u * (center.x + radius),
            coordinate - center.y,
        );
    }

    faded_line(
        origin + u * (center.x - radius),
        origin + u * (center.x + radius),
        Color::srgba(0.80, 0.25, 0.30, 0.62),
        fade(center.y),
    );
    faded_line(
        origin + v * (center.y - radius),
        origin + v * (center.y + radius),
        Color::srgba(0.25, 0.65, 0.38, 0.62),
        fade(center.x),
    );
}

fn draw_plane_outline<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    half_size: f32,
    color: Color,
) {
    let origin = basis_vector(basis.origin);
    let u = basis_vector(basis.u) * half_size;
    let v = basis_vector(basis.v) * half_size;
    let corners = [
        origin - u - v,
        origin + u - v,
        origin + u + v,
        origin - u + v,
    ];
    for index in 0..4 {
        gizmos.line(corners[index], corners[(index + 1) % 4], color);
    }
    gizmos.line(origin - u, origin + u, color.with_alpha(0.46));
    gizmos.line(origin - v, origin + v, color.with_alpha(0.46));
}

/// One planar face beside a model edge, in world space: its outward normal and
/// a point inside it, enough to tell whether the face rises towards the camera
/// as it leaves the edge.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EdgeSideFace {
    normal: Vec3,
    interior: Vec3,
}

fn transform_edge_sides(
    sides: [Option<EdgeSideFace>; 2],
    transform: &Transform,
) -> [Option<EdgeSideFace>; 2] {
    sides.map(|side| {
        side.map(|side| EdgeSideFace {
            normal: (transform.rotation * side.normal).normalize_or_zero(),
            interior: transform.transform_point(side.interior),
        })
    })
}

/// The planar faces beside each edge of a body, by edge key. Curved faces are
/// left out; an edge beside one keeps the plain tie-break lift.
fn edge_side_faces<'a>(
    body: &'a BodyDto,
    transform: &Transform,
) -> HashMap<&'a str, [Option<EdgeSideFace>; 2]> {
    let mut sides: HashMap<&str, [Option<EdgeSideFace>; 2]> = HashMap::new();
    for face in &body.faces {
        let (Some(plane), Some(signature)) = (&face.plane, &face.signature) else {
            continue;
        };
        let normal = (transform.rotation * basis_vector(plane.normal)).normalize_or_zero();
        if normal == Vec3::ZERO {
            continue;
        }
        let side = EdgeSideFace {
            normal,
            interior: transform.transform_point(Vec3::new(
                signature.centroid.x as f32,
                signature.centroid.y as f32,
                signature.centroid.z as f32,
            )),
        };
        for key in &face.edge_keys {
            let entry = sides.entry(key.as_str()).or_default();
            if entry[0].is_none() {
                entry[0] = Some(side);
            } else if entry[1].is_none() {
                entry[1] = Some(side);
            }
        }
    }
    sides
}

/// How far a model-edge stroke is nudged towards the camera so it wins the
/// depth comparison against the faces it lies on.
#[derive(Clone, Copy)]
struct EdgeLift {
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    sides: [Option<EdgeSideFace>; 2],
    /// Hard ceiling in model units, from the body's size.
    ceiling: f32,
}

/// Extra depth, in pixels, that a stroke half `half_width_px` wide needs so
/// its outer pixels are not hidden by a face beside the edge that rises
/// towards the camera. Zero for faces that fall away, as both faces of a
/// convex edge do; both faces of an inside corner rise, which is what turned
/// concave edges into faint dashes while convex ones stayed solid.
fn edge_stroke_rise_px(
    along: Vec3,
    middle: Vec3,
    forward: Vec3,
    sides: &[Option<EdgeSideFace>; 2],
    half_width_px: f32,
) -> f32 {
    let mut rise = 0.0f32;
    for side in sides.iter().flatten() {
        let mut across = side.normal.cross(along).normalize_or_zero();
        if across == Vec3::ZERO {
            continue;
        }
        if (side.interior - middle).dot(across) < 0.0 {
            across = -across;
        }

        let sine = across.dot(forward);
        if sine >= 0.0 {
            continue;
        }
        let cosine = (1.0 - sine * sine).max(1.0e-4).sqrt();
        rise = rise.max(-sine / cosine * half_width_px);
    }
    rise
}

fn draw_edge_segments<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    edge: &limo_cad_solid::EdgeDto,
    color: Color,
    transform: &Transform,
    lift: Option<EdgeLift>,
) {
    let lift = lift.map(|lift| {
        let position = Vec3::from_array(lift.camera.position);
        let forward = (Vec3::from_array(lift.camera.target) - position).normalize_or_zero();
        (forward, lift)
    });
    for pair in edge.points.windows(2) {
        let mut start = transform.transform_point(Vec3::new(
            pair[0].x as f32,
            pair[0].y as f32,
            pair[0].z as f32,
        ));
        let mut end = transform.transform_point(Vec3::new(
            pair[1].x as f32,
            pair[1].y as f32,
            pair[1].z as f32,
        ));
        if let Some((forward, lift)) = lift {
            let middle = (start + end) * 0.5;
            let pixel = world_per_pixel_at(lift.camera, lift.viewport, middle);
            let mut distance = pixel.min(MODEL_EDGE_MAX_LIFT_MM);
            let along = (end - start).normalize_or_zero();
            if along != Vec3::ZERO {
                let rise = edge_stroke_rise_px(
                    along,
                    middle,
                    forward,
                    &lift.sides,
                    MODEL_EDGE_STROKE_HALF_WIDTH_PX,
                );
                if rise > 0.0 {
                    distance =
                        distance.max((rise.min(MODEL_EDGE_MAX_LIFT_PX) * pixel).min(lift.ceiling));
                }
            }
            let offset = forward * distance;
            start -= offset;
            end -= offset;
        }
        gizmos.line(start, end, color);
    }
}

fn edge_is_straight(edge: &limo_cad_solid::EdgeDto) -> bool {
    limo_cad_solid::edge_is_straight(edge)
}

fn face_boundary_segments(body: &BodyDto, face: &FaceDto) -> Vec<(Vec3, Vec3)> {
    let start = face.first_index as usize;
    let end = start
        .saturating_add(face.index_count as usize)
        .min(body.mesh.indices.len());
    triangle_boundary_segments(&body.mesh.positions, &body.mesh.indices[start..end])
}

#[derive(Clone, Copy)]
struct BoundarySegment {
    count: u32,
    start: Vec3,
    end: Vec3,
}

fn triangle_boundary_segments(positions: &[f32], indices: &[u32]) -> Vec<(Vec3, Vec3)> {
    let point = |index: u32| {
        let offset = index as usize * 3;
        let value = positions.get(offset..offset + 3)?;
        Some(Vec3::new(value[0], value[1], value[2]))
    };
    let point_key = |value: Vec3| {
        [
            (value.x * 1_000_000.0).round() as i64,
            (value.y * 1_000_000.0).round() as i64,
            (value.z * 1_000_000.0).round() as i64,
        ]
    };
    let mut segments = HashMap::<([i64; 3], [i64; 3]), BoundarySegment>::new();
    for triangle in indices.as_chunks::<3>().0 {
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let (Some(start), Some(end)) = (point(a), point(b)) else {
                continue;
            };
            let start_key = point_key(start);
            let end_key = point_key(end);
            if start_key == end_key {
                continue;
            }
            let key = if start_key <= end_key {
                (start_key, end_key)
            } else {
                (end_key, start_key)
            };
            segments
                .entry(key)
                .and_modify(|segment| segment.count += 1)
                .or_insert(BoundarySegment {
                    count: 1,
                    start,
                    end,
                });
        }
    }
    segments
        .into_values()
        .filter_map(|segment| (segment.count == 1).then_some((segment.start, segment.end)))
        .collect()
}

fn sketch_entity_style(entity: &EntityDto) -> (u64, bool) {
    match entity {
        EntityDto::Point {
            id, fully_defined, ..
        }
        | EntityDto::Line {
            id, fully_defined, ..
        }
        | EntityDto::Arc {
            id, fully_defined, ..
        }
        | EntityDto::Circle {
            id, fully_defined, ..
        }
        | EntityDto::Spline {
            id, fully_defined, ..
        } => (id.0, *fully_defined),
    }
}

fn draw_sketch<Config, ColorFor>(
    gizmos: &mut Gizmos<Config>,
    sketch: &SketchDto,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    point_radius_px: f32,
    mut color_for: ColorFor,
    show_points: bool,
) where
    Config: GizmoConfigGroup,
    ColorFor: FnMut(&EntityDto) -> Option<Color>,
{
    for entity in &sketch.entities {
        let Some(color) = color_for(entity) else {
            continue;
        };
        draw_sketch_curve(gizmos, &sketch.basis, entity, color);
        if show_points {
            draw_sketch_entity_grips(
                gizmos,
                &sketch.basis,
                entity,
                (camera, viewport),
                point_radius_px,
                color,
                !sketch_entity_style(entity).1,
            );
        }
    }
}

/// Draw the support-face boundary projected into the active sketch.
///
/// This is reference geometry the user cannot pick, hover, grip or constrain,
/// so it gets its own color and sits just under the authored sketch strokes
/// (`FINISHED_SKETCH_OFFSET`). Use the same directed tessellation as browser
/// drawing and snapping. Endpoint angles alone cannot distinguish a clockwise
/// partial edge from its complementary arc on a reversed face basis. The exact
/// circular carrier remains available to profile extraction and the kernel.
fn draw_projected_edges<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    sketch: &SketchDto,
    color: Color,
) {
    for edge in &sketch.projected_edges {
        for pair in edge.points.windows(2) {
            gizmos.line(
                sketch_world(&sketch.basis, pair[0].x, pair[0].y, FINISHED_SKETCH_OFFSET),
                sketch_world(&sketch.basis, pair[1].x, pair[1].y, FINISHED_SKETCH_OFFSET),
                color,
            );
        }
    }
}

fn draw_base_curve_outside_profiles<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    entity: &EntityDto,
    color: Color,
    profiles: &[&ProfileLoopDto],
) {
    match base_curve_remainder(entity, profiles) {
        BaseCurveRemainder::Complete => draw_sketch_curve(gizmos, basis, entity, color),
        BaseCurveRemainder::Lines(segments) => {
            for [start, end] in segments {
                gizmos.line(
                    sketch_world(basis, start.x, start.y, FINISHED_SKETCH_OFFSET),
                    sketch_world(basis, end.x, end.y, FINISHED_SKETCH_OFFSET),
                    color,
                );
            }
        }
        BaseCurveRemainder::Circular(ranges) => {
            let (center, radius) = match entity {
                EntityDto::Arc { center, radius, .. }
                | EntityDto::Circle { center, radius, .. } => (center, radius),
                _ => return,
            };
            for [start, end] in ranges {
                let sweep = end - start;
                let segments = ((sweep.abs() * 20.0).ceil() as usize).clamp(12, 128);
                draw_parametric_curve(gizmos, segments, color, |ratio| {
                    let angle = start + sweep * ratio;
                    sketch_world(
                        basis,
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                        FINISHED_SKETCH_OFFSET,
                    )
                });
            }
        }
    }
}

fn draw_sketch_curve<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    entity: &EntityDto,
    color: Color,
) {
    draw_sketch_curve_at_offset(gizmos, basis, entity, color, FINISHED_SKETCH_OFFSET);
}

fn draw_sketch_curve_at_offset<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    entity: &EntityDto,
    color: Color,
    offset: f32,
) {
    match entity {
        EntityDto::Point { .. } => {}
        EntityDto::Line { start, end, .. } => {
            gizmos.line(
                sketch_world(basis, start.x, start.y, offset),
                sketch_world(basis, end.x, end.y, offset),
                color,
            );
        }
        EntityDto::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        } => {
            let mut sweep = end_angle - start_angle;
            while sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            let segments = ((sweep.abs() * 20.0).ceil() as usize).clamp(12, 128);
            draw_parametric_curve(gizmos, segments, color, |ratio| {
                let angle = start_angle + sweep * ratio;
                sketch_world(
                    basis,
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                    offset,
                )
            });
        }
        EntityDto::Circle { center, radius, .. } => {
            draw_parametric_curve(gizmos, 72, color, |ratio| {
                let angle = std::f64::consts::TAU * ratio;
                sketch_world(
                    basis,
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                    offset,
                )
            });
        }
        EntityDto::Spline { tessellation, .. } => {
            for pair in tessellation.windows(2) {
                gizmos.line(
                    sketch_world(basis, pair[0].x, pair[0].y, offset),
                    sketch_world(basis, pair[1].x, pair[1].y, offset),
                    color,
                );
            }
        }
    }
}

fn draw_sketch_entity_grips<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    entity: &EntityDto,
    (camera, viewport): (ViewportCamera, ViewportSizeResource),
    point_radius_px: f32,
    color: Color,
    hollow: bool,
) {
    for position in sketch_grip_positions(entity) {
        draw_sketch_grip(
            gizmos,
            basis,
            position,
            (camera, viewport),
            point_radius_px,
            color,
            hollow,
        );
    }
}

fn sketch_grip_positions(entity: &EntityDto) -> &[SketchVec2] {
    match entity {
        EntityDto::Point { position, .. } => std::slice::from_ref(position),
        EntityDto::Arc { center, .. } | EntityDto::Circle { center, .. } => {
            std::slice::from_ref(center)
        }
        EntityDto::Spline { points, .. } => points,
        EntityDto::Line { .. } => &[],
    }
}

fn referenced_sketch_point<'a>(
    entity: &'a EntityDto,
    reference: &SketchPointRefDto,
) -> Option<&'a SketchVec2> {
    match (entity, &reference.point) {
        (EntityDto::Point { position, .. }, SketchPointKindDto::Point) => Some(position),
        (EntityDto::Line { start, .. }, SketchPointKindDto::Start) => Some(start),
        (EntityDto::Line { end, .. }, SketchPointKindDto::End) => Some(end),
        (EntityDto::Arc { center, .. }, SketchPointKindDto::Center)
        | (EntityDto::Circle { center, .. }, SketchPointKindDto::Center) => Some(center),
        (EntityDto::Spline { points, .. }, SketchPointKindDto::FitPoint { index }) => {
            points.get(*index as usize)
        }
        _ => None,
    }
}

fn draw_sketch_grip<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    position: &SketchVec2,
    (camera, viewport): (ViewportCamera, ViewportSizeResource),
    point_radius_px: f32,
    color: Color,
    hollow: bool,
) {
    let point = sketch_world(basis, position.x, position.y, 0.05);
    let radius = screen_space_disc_radius(camera, viewport, point, point_radius_px);
    let (right, up) = camera_facing_axes(camera);
    if hollow {
        let segments = 24;
        for index in 0..segments {
            let angle = index as f32 / segments as f32 * std::f32::consts::TAU;
            let next_angle = (index + 1) as f32 / segments as f32 * std::f32::consts::TAU;
            let start = point + right * (angle.cos() * radius) + up * (angle.sin() * radius);
            let end =
                point + right * (next_angle.cos() * radius) + up * (next_angle.sin() * radius);
            gizmos.line(start, end, color);
        }
    } else {
        draw_filled_disc(gizmos, point, right, up, radius, color, 4);
    }
}

/// Hole positions are the sketch points projected onto the support face, so
/// their pick markers sit on that face. Without a support face the marker
/// stays on the point's own sketch plane.
fn hole_point_marker_world(
    basis: &PlaneBasis,
    position: &SketchVec2,
    support: Option<&PlaneBasis>,
) -> Vec3 {
    let Some(support) = support else {
        return sketch_world(basis, position.x, position.y, 0.05);
    };
    let world = basis.to_3d([position.x, position.y]);
    let normal = support.normal;
    let offset = (0..3)
        .map(|axis| (world[axis] - support.origin[axis]) * normal[axis])
        .sum::<f64>();
    let projected = [
        world[0] - normal[0] * offset,
        world[1] - normal[1] * offset,
        world[2] - normal[2] * offset,
    ];
    Vec3::new(
        projected[0] as f32,
        projected[1] as f32,
        projected[2] as f32,
    ) + basis_vector(support.normal) * 0.05
}

fn draw_screen_dot<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    point: Vec3,
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    point_radius_px: f32,
    color: Color,
) {
    let radius = screen_space_disc_radius(camera, viewport, point, point_radius_px);
    let (right, up) = camera_facing_axes(camera);
    draw_filled_disc(gizmos, point, right, up, radius, color, 4);
}

/// Gizmos do not expose a filled world-space disc primitive. A handful of
/// parallel chords gives sketch points a true round-dot silhouette while
/// keeping their physical diameter tied to line weight on Retina and
/// standard-density displays. `half_steps` controls chord density: small
/// sketch grips stay cheap at 4, while large CAM pick markers pass one row
/// per logical pixel so the fill reads solid instead of striped.
fn draw_filled_disc<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    center: Vec3,
    u_axis: Vec3,
    v_axis: Vec3,
    radius: f32,
    color: Color,
    half_steps: i32,
) {
    let u = u_axis.normalize_or_zero();
    let v = v_axis.normalize_or_zero();
    if u == Vec3::ZERO || v == Vec3::ZERO {
        return;
    }
    for step in -half_steps..=half_steps {
        let ratio = step as f32 / half_steps as f32;
        let along_v = ratio * radius;
        let half_chord = (radius * radius - along_v * along_v).max(0.0).sqrt();
        let row_center = center + v * along_v;
        gizmos.line(
            row_center - u * half_chord,
            row_center + u * half_chord,
            color,
        );
    }
}

fn camera_facing_axes(camera: ViewportCamera) -> (Vec3, Vec3) {
    let camera_position = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - camera_position).normalize_or_zero();
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let mut right = forward.cross(up_hint).normalize_or_zero();
    if right.length_squared() < 1.0e-8 {
        right = Vec3::X;
    }
    let mut up = right.cross(forward).normalize_or_zero();
    if up.length_squared() < 1.0e-8 {
        up = Vec3::Y;
    }
    (right, up)
}

fn screen_space_disc_radius(
    camera: ViewportCamera,
    viewport: ViewportSizeResource,
    center: Vec3,
    radius_px: f32,
) -> f32 {
    let world_per_pixel = world_per_pixel_at(camera, viewport, center);
    world_per_pixel * radius_px.max(1.0)
}

fn draw_parametric_curve(
    gizmos: &mut Gizmos<impl GizmoConfigGroup>,
    segments: usize,
    color: Color,
    point: impl Fn(f64) -> Vec3,
) {
    let mut previous = point(0.0);
    for index in 1..=segments {
        let next = point(index as f64 / segments as f64);
        gizmos.line(previous, next, color);
        previous = next;
    }
}

fn sketch_world(basis: &PlaneBasis, x: f64, y: f64, offset: f32) -> Vec3 {
    Vec3::new(
        (basis.origin[0] + basis.u[0] * x + basis.v[0] * y) as f32,
        (basis.origin[1] + basis.u[1] * x + basis.v[1] * y) as f32,
        (basis.origin[2] + basis.u[2] * x + basis.v[2] * y) as f32,
    ) + basis_vector(basis.normal) * offset
}

fn draw_profile_segments<Config: GizmoConfigGroup>(
    gizmos: &mut Gizmos<Config>,
    basis: &PlaneBasis,
    segments: &[[Point2Dto; 2]],
    color: Color,
) {
    for [start, end] in segments {
        gizmos.line(
            sketch_world(basis, start.x, start.y, PROFILE_PICK_OFFSET),
            sketch_world(basis, end.x, end.y, PROFILE_PICK_OFFSET),
            color,
        );
    }
}

fn basis_vector(vector: [f64; 3]) -> Vec3 {
    Vec3::new(vector[0] as f32, vector[1] as f32, vector[2] as f32)
}

fn rgb(value: [f32; 3]) -> Color {
    Color::srgb(value[0], value[1], value[2])
}

fn rgba(value: [f32; 3], alpha: f32) -> Color {
    Color::srgba(value[0], value[1], value[2], alpha)
}

fn bind_document_geometry(world: &mut World) {
    world.init_resource::<DocumentGeometryIndex>();
    let model = world.resource::<ModelResource>();
    let existing = world
        .resource::<DocumentGeometryIndex>()
        .0
        .get(&model.session_id)
        .copied();
    let scene = Arc::clone(&model.document.scene);
    let cache_entity = if let Some(entity) = existing {
        entity
    } else {
        let session_id = world.resource::<ModelResource>().session_id.clone();
        let entity = world
            .spawn((ModelEdgeCache::default(), BodyMeshCache::default()))
            .id();
        world
            .resource_mut::<DocumentGeometryIndex>()
            .0
            .insert(session_id, entity);
        entity
    };
    world.resource_mut::<ModelResource>().cache_entity = Some(cache_entity);
    if !std::ptr::eq(
        world
            .get::<ModelEdgeCache>(cache_entity)
            .expect("document edge cache")
            .scene
            .as_ptr(),
        Arc::as_ptr(&scene),
    ) {
        world
            .get_mut::<ModelEdgeCache>(cache_entity)
            .expect("document edge cache")
            .update(&scene);
    }
}

fn apply_model_state(world: &mut World, next: ViewportModel, update: InstanceUpdate) {
    let mut resource = world.resource_mut::<ModelResource>();
    resource.bind_instance_state(&next.session_id, &next.instance_body_poses, update);
    let reset_sketch =
        resource.session_id != next.session_id || next.document.active_sketch.is_none();
    resource.session_id = next.session_id;
    resource.geometry_revision = next.geometry_revision;
    resource.document = next.document;
    resource.body_poses = next.body_poses;
    resource.instance_body_poses = next.instance_body_poses;
    resource.revision = resource.revision.wrapping_add(1);
    bind_document_geometry(world);
    if reset_sketch {
        if let Some(mut preview) = world.get_resource_mut::<PreviewResource>() {
            preview.sketch_lines.clear();
        }
    }
    invalidate_interface_presentation(world);
}

/// Install the same renderer/picker state for a Winit-owned window. The CAD
/// systems and retained geometry cache are shared with headless fixtures.
pub(super) fn install_native_scene(app: &mut bevy::app::App) {
    install_cad_scene(app);
    app.insert_resource(SharedPickState(Arc::new(Mutex::new(PickState::default()))));
}

/// Production scene initialization without starting an OS window or renderer.
/// Tests inspect the same authoritative resources used by the native host.
#[cfg(test)]
pub(crate) fn interface_scene_fixture() -> bevy::app::App {
    let mut app = bevy::app::App::new();
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::asset::AssetPlugin::default(),
    ));
    install_native_scene(&mut app);
    app
}

/// Run the production mesh update without a renderer, then report retained
/// entity and strong asset identities. This measures lifecycle, not GPU memory
/// or frame latency; no OS window, graphics adapter or input loop is created.
#[cfg(test)]
pub(crate) fn interface_geometry_fixture_snapshot(world: &mut World) -> serde_json::Value {
    use bevy::ecs::system::RunSystemOnce;
    #[derive(Resource)]
    struct GeometryFixtureReady;
    if !world.contains_resource::<GeometryFixtureReady>() {
        world.register_required_components::<Mesh3d, Visibility>();
        world.insert_resource(GeometryFixtureReady);
    }
    world.init_resource::<Assets<Mesh>>();
    world.init_resource::<Assets<StandardMaterial>>();
    world.run_system_once(rebuild_occt_meshes).unwrap();
    world.flush();
    let mut sessions = std::collections::BTreeMap::<String, Vec<serde_json::Value>>::new();
    let mut query = world.query::<(
        Entity,
        &NativeModelGeometry,
        Option<&Mesh3d>,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>();
    for (entity, geometry, mesh, material) in query.iter(world) {
        sessions
            .entry(geometry.session_id.clone())
            .or_default()
            .push(serde_json::json!({
                "entity": entity.to_bits(),
                "mesh": mesh.map(|mesh| format!("{:?}", mesh.0.id())),
                "material": material.map(|material| format!("{:?}", material.0.id())),
            }));
    }
    for rows in sessions.values_mut() {
        rows.sort_by_key(|row| row["entity"].as_u64().unwrap());
    }
    let cache: std::collections::BTreeMap<_, _> = world
        .resource::<DocumentGeometryIndex>()
        .0
        .iter()
        .filter_map(|(session, entity)| {
            world
                .get::<BodyMeshCache>(*entity)
                .and_then(|cache| cache.rendered)
                .map(|stamp| (session, stamp))
        })
        .collect();
    serde_json::json!({"sessions": sessions, "cache": cache})
}

pub(crate) fn interface_preview_snapshot(world: &World) -> Arc<ViewportPreview> {
    world.resource::<PreviewResource>().value.clone()
}

pub(crate) fn interface_preview_revision(world: &World) -> u64 {
    world.resource::<PreviewResource>().revision
}

pub(crate) fn interface_pick(
    world: &World,
    session_id: &str,
    point: [f32; 2],
    purpose: NativePickPurpose,
) -> Result<Option<NativePick>, String> {
    // Temporary cap topology is inspection evidence, never a modeling target.
    if section_view::active(world) {
        return Ok(None);
    }
    let model = world.resource::<ModelResource>();
    if model.session_id != session_id {
        return Err("Native viewport has not bound the requested document".into());
    }
    let size = world.resource::<ViewportSizeResource>();
    Ok(pick_occt_scene(
        &model.document.scene,
        (
            world.resource::<CameraResource>().camera,
            (size.logical_width, size.logical_height),
            point[0],
            point[1],
        ),
        &world.resource::<PresentationResource>().0.hidden_body_ids,
        &model.body_poses,
        &model.instance_body_poses,
        purpose,
    ))
}

/// Pick the visible support using the same trimmed faces, camera ray and
/// screen-sized reference quads as the renderer. Construction overlays win
/// before origin overlays, then trimmed faces, as in the original picker.
pub(crate) fn interface_support_pick(
    world: &World,
    session_id: &str,
    point: [f32; 2],
) -> Result<Option<limo_cad_core::PlaneRef>, String> {
    use limo_cad_core::PlaneRef;
    let model = world.resource::<ModelResource>();
    if model.session_id != session_id {
        return Err("Native viewport has not bound the requested document".into());
    }
    let size = *world.resource::<ViewportSizeResource>();
    let camera = world.resource::<CameraResource>().camera;
    let state = &world.resource::<PresentationResource>().0;
    let Some((origin, direction, _)) = camera_pick_ray(
        camera,
        (size.logical_width, size.logical_height),
        point[0],
        point[1],
    ) else {
        return Ok(None);
    };
    let mut distance = f32::INFINITY;
    let mut result = None;
    let mut check = |reference, basis: PlaneBasis| {
        let half = reference_plane_half_size(
            camera,
            size,
            Vec3::from_array(basis.origin.map(|v| v as f32)),
        );
        if let Some(t) = ray_reference_quad(origin, direction, basis, half) {
            if t < distance {
                distance = t;
                result = Some(reference);
            }
        }
    };
    for plane in &model.document.datum_planes {
        if !state.hidden_datum_plane_ids.contains(&plane.datum_id.0) {
            check(
                PlaneRef::DatumPlane {
                    datum_id: plane.datum_id,
                },
                plane.basis,
            );
        }
    }
    drop(check);
    if result.is_some() {
        return Ok(result);
    }
    for reference in PlaneRef::ORIGIN_PLANES {
        let basis = reference.origin_basis().unwrap();
        let half = reference_plane_half_size(camera, size, Vec3::ZERO);
        if let Some(t) = ray_reference_quad(origin, direction, basis, half) {
            if t < distance {
                distance = t;
                result = Some(reference);
            }
        }
    }
    if result.is_some() {
        return Ok(result);
    }
    let hit = interface_pick(world, session_id, point, NativePickPurpose::Geometry)?;
    Ok(hit.as_ref().and_then(|h| {
        model
            .document
            .scene
            .bodies
            .iter()
            .find(|b| b.id.0 == h.body_id)?
            .faces
            .iter()
            .find(|f| f.id.0 == h.face_id && f.plane.is_some())
            .map(|f| PlaneRef::PlanarFace { face_id: f.id })
    }))
}

fn ray_reference_quad(origin: Vec3, direction: Vec3, basis: PlaneBasis, half: f32) -> Option<f32> {
    let normal = Vec3::from_array(basis.normal.map(|v| v as f32));
    let denominator = direction.dot(normal);
    if !denominator.is_finite() || denominator.abs() < 1e-8 || !half.is_finite() || half <= 0. {
        return None;
    }
    let distance =
        (Vec3::from_array(basis.origin.map(|v| v as f32)) - origin).dot(normal) / denominator;
    if !distance.is_finite() || distance < 0. {
        return None;
    }
    let local = basis.to_2d((origin + direction * distance).to_array().map(f64::from));
    (local
        .iter()
        .all(|v| v.is_finite() && v.abs() <= f64::from(half)))
    .then_some(distance)
}

/// Apply a transient layer only under its live document's owner guard. A stale
/// form cannot clear or restore another document's in-progress presentation.
pub(crate) fn apply_interface_preview(
    world: &mut World,
    session_id: &str,
    preview: impl Into<Arc<ViewportPreview>>,
) -> Result<(), String> {
    if world.resource::<ModelResource>().session_id != session_id {
        return Err("Native viewport has not bound the requested document".into());
    }
    let preview = preview.into();
    if Arc::ptr_eq(&world.resource::<PreviewResource>().value, &preview) {
        return Ok(());
    }
    validate_preview(&preview)?;
    apply_preview_state(world, preview);
    Ok(())
}

/// Retained stock is guarded by the document owner before a background
/// simulation can become visible.
pub(crate) fn interface_cam_stock_snapshot(world: &World) -> (u64, Option<ViewportCamStock>) {
    let resource = world.resource::<CamStockResource>();
    (resource.revision, resource.value.clone())
}

pub(crate) fn apply_interface_cam_stock(
    world: &mut World,
    session_id: &str,
    stock: Option<ViewportCamStock>,
) -> Result<(), String> {
    if world.resource::<ModelResource>().session_id != session_id {
        return Err("CAM stock belongs to a retired document".into());
    }
    validate_cam_stock(stock.as_ref())?;
    world.init_resource::<CamStockResource>();
    let mut resource = world.resource_mut::<CamStockResource>();
    resource.value = stock;
    resource.revision = resource.revision.wrapping_add(1);

    invalidate_interface_presentation(world);
    Ok(())
}

pub(crate) fn apply_interface_sketch_lines(
    world: &mut World,
    session_id: &str,
    lines: Vec<super::ViewportLineLayer>,
) -> Result<(), String> {
    if world.resource::<ModelResource>().session_id != session_id {
        return Err("Sketch annotations belong to a retired document".into());
    }
    world.resource_mut::<PreviewResource>().sketch_lines = lines;
    invalidate_interface_presentation(world);
    Ok(())
}

fn apply_preview_state(world: &mut World, preview: Arc<ViewportPreview>) {
    let mut resource = world.resource_mut::<PreviewResource>();
    if preview_mesh_content_changed(&resource.value, &preview) {
        resource.mesh_revision = resource.mesh_revision.wrapping_add(1);
    }
    resource.value = preview;
    resource.revision = resource.revision.wrapping_add(1);

    invalidate_interface_presentation(world);
}

fn invalidate_interface_presentation(world: &World) {
    if let Some(handle) = world.get_resource::<NativeInterfaceHandle>() {
        handle.invalidate_presentation();
    }
}

/// Called only while the native document publisher owns this exact update.
/// Both renderer hosts share this state reducer; no second geometry path.
pub(crate) fn apply_interface_model(world: &mut World, next: ViewportModel) -> Result<(), String> {
    apply_interface_model_state(world, next, InstanceUpdate::LiveModel)
}

fn apply_interface_model_state(
    world: &mut World,
    next: ViewportModel,
    update: InstanceUpdate,
) -> Result<(), String> {
    let picker = world
        .get_resource::<SharedPickState>()
        .ok_or("Native picker is unavailable")?
        .0
        .clone();
    let mut picker = picker.lock().map_err(|_| "Native picker lock poisoned")?;
    picker.scene = Arc::clone(&next.document.scene);
    picker.body_poses.clone_from(&next.body_poses);
    picker
        .instance_body_poses
        .clone_from(&next.instance_body_poses);
    apply_model_state(world, next, update);
    Ok(())
}

/// A real pre-feature model prepared in an isolated kernel. Its local geometry
/// counter cannot authorize reuse of meshes from the live document (or vice versa).
pub(crate) fn apply_interface_edit_model(
    world: &mut World,
    next: ViewportModel,
) -> Result<(), String> {
    apply_interface_model_state(world, next, InstanceUpdate::IsolatedModel)
}

/// Refresh the existing renderer's materials, grid and HUD together. Geometry
/// intent and the engine revision are untouched by application appearance.
pub(crate) fn apply_interface_palette(world: &mut World, palette: ViewportPalette) {
    if world.resource::<PaletteResource>().0 == palette {
        return;
    }
    *world.resource_mut::<ClearColor>() = ClearColor(rgb(palette.background));
    world.resource_mut::<PaletteResource>().0 = palette;
    let mut model = world.resource_mut::<ModelResource>();
    model.revision = model.revision.wrapping_add(1);

    let mut hud = world.resource_mut::<HudResource>();
    hud.revision = hud.revision.wrapping_add(1);
}

pub(crate) fn apply_interface_selection_readout(
    world: &mut World,
    selection: Option<super::ViewportHudSelection>,
) {
    let mut hud = world.resource_mut::<HudResource>();
    if hud.hud.selection != selection {
        hud.hud.selection = selection;
        hud.revision = hud.revision.wrapping_add(1);
    }
}

/// Borrow presentation state for read-only controls and view calculations.
pub(crate) fn interface_view(
    world: &World,
) -> (&str, ViewportCamera, &ViewportPresentation, [f32; 2]) {
    let size = world.resource::<ViewportSizeResource>();
    (
        &world.resource::<ModelResource>().session_id,
        world.resource::<CameraResource>().camera,
        &world.resource::<PresentationResource>().0,
        [size.logical_width, size.logical_height],
    )
}

/// Capture owned state only when a control needs to edit or retain it.
pub(crate) fn interface_view_snapshot(
    world: &World,
) -> (String, ViewportCamera, ViewportPresentation, [f32; 2]) {
    let (document, camera, presentation, size) = interface_view(world);
    (document.to_owned(), camera, presentation.clone(), size)
}

/// Camera motion samples never clone selection or assembly-pose vectors.
pub(crate) fn interface_camera_snapshot(world: &World) -> (String, ViewportCamera) {
    (
        world.resource::<ModelResource>().session_id.clone(),
        world.resource::<CameraResource>().camera,
    )
}

pub(crate) fn interface_model_revision(world: &World) -> u64 {
    world.resource::<ModelResource>().revision
}

/// Borrow only the navigation sources; camera updates do not invalidate this
/// stamp and motion never clones selection arrays or document geometry.
pub(crate) fn interface_navigation_source(world: &World) -> ([u32; 2], &ViewportPresentation) {
    let model = world
        .get_resource_ref::<ModelResource>()
        .expect("rendered model");
    let presentation = world
        .get_resource_ref::<PresentationResource>()
        .expect("presentation");
    let stamp = [
        model.last_changed().get(),
        presentation.last_changed().get(),
    ];
    (stamp, &world.resource::<PresentationResource>().0)
}

pub(crate) fn interface_geometry(world: &World) -> super::ViewportGeometry<'_> {
    let model = world.resource::<ModelResource>();
    super::ViewportGeometry {
        scene: &model.document.scene,
        active_sketch: model.document.active_sketch.as_ref(),
        finished_sketches: &model.document.finished_sketches,
        instance_body_poses: &model.instance_body_poses,
    }
}

/// Borrow cached source-definition bounds without scanning or cloning geometry.
/// A cache from another document or scene cannot supply an inspection default.
pub(crate) fn interface_body_local_center(
    world: &World,
    session_id: &str,
    scene: &Arc<SolidSceneDto>,
    body_id: u64,
) -> Option<[f32; 3]> {
    let model = world.get_resource::<ModelResource>()?;
    if model.session_id != session_id || !Arc::ptr_eq(&model.document.scene, scene) {
        return None;
    }
    let cache = world.get::<ModelEdgeCache>(model.cache_entity?)?;
    if !std::ptr::eq(cache.scene.as_ptr(), Arc::as_ptr(scene)) {
        return None;
    }
    cache
        .bodies
        .get(&body_id)?
        .local_bounds
        .map(|(center, _)| center.to_array())
}

pub(crate) fn interface_body_transform(
    world: &World,
    body_id: u64,
    occurrence_id: Option<u64>,
) -> Transform {
    let model = world.resource::<ModelResource>();
    instance_body_pose_transform(
        &model.instance_body_poses,
        &model.body_poses,
        body_id,
        occurrence_id,
    )
}

pub(crate) fn interface_visible_occurrences(world: &World, body_id: u64) -> Vec<Option<u64>> {
    visible_body_occurrences(world.resource::<ModelResource>(), body_id)
}

pub(crate) fn interface_sketch_point(
    world: &World,
    session_id: &str,
    point: [f32; 2],
    basis: PlaneBasis,
) -> Result<Option<limo_cad_sketch::Vec2>, String> {
    if world.resource::<ModelResource>().session_id != session_id {
        return Err("Native viewport has not bound the requested document".into());
    }
    if basis
        .origin
        .iter()
        .chain(&basis.u)
        .chain(&basis.v)
        .chain(&basis.normal)
        .any(|value| !value.is_finite())
    {
        return Err("Sketch plane must be finite".into());
    }
    let size = world.resource::<ViewportSizeResource>();
    let camera = world.resource::<CameraResource>().camera;
    let Some((origin, direction, _)) = camera_pick_ray(
        camera,
        (size.logical_width, size.logical_height),
        point[0],
        point[1],
    ) else {
        return Ok(None);
    };
    let normal = bevy::math::DVec3::from_array(basis.normal);
    let origin = origin.as_dvec3();
    let direction = direction.as_dvec3();
    let denominator = normal.dot(direction);
    if !denominator.is_finite() || denominator.abs() <= 1.0e-10 {
        return Ok(None);
    }
    let distance = normal.dot(bevy::math::DVec3::from_array(basis.origin) - origin) / denominator;
    if !distance.is_finite() || distance < 0.0 {
        return Ok(None);
    }
    let point = basis.to_2d((origin + direction * distance).to_array());
    Ok(point
        .iter()
        .all(|value| value.is_finite())
        .then(|| limo_cad_sketch::Vec2::new(point[0], point[1])))
}

/// Project through the same camera basis used for CAD picking. Coordinates are
/// viewport-local logical pixels; the host adds its actual canvas origin.
pub(crate) fn interface_world_point(
    world: &World,
    session_id: &str,
    point: [f64; 3],
) -> Result<Option<[f32; 2]>, String> {
    if world.resource::<ModelResource>().session_id != session_id {
        return Err("Native viewport has not bound the requested document".into());
    }
    if point.iter().any(|value| !value.is_finite()) {
        return Err("World point must be finite".into());
    }
    let size = world.resource::<ViewportSizeResource>();
    let viewport = (size.logical_width, size.logical_height);
    let camera = world.resource::<CameraResource>().camera;
    let Some(basis) = camera_projection(camera, viewport) else {
        return Ok(None);
    };
    let offset = bevy::math::DVec3::from_array(point) - basis.origin.as_dvec3();
    let depth = offset.dot(basis.forward.as_dvec3());
    if depth <= 0.0 || !depth.is_finite() {
        return Ok(None);
    }
    let ndc_x =
        offset.dot(basis.right.as_dvec3()) / (depth * f64::from(basis.tangent * basis.aspect));
    let ndc_y = offset.dot(basis.up.as_dvec3()) / (depth * f64::from(basis.tangent));
    let pixel = [
        ((ndc_x + 1.0) * 0.5 * f64::from(viewport.0)) as f32,
        ((1.0 - ndc_y) * 0.5 * f64::from(viewport.1)) as f32,
    ];
    Ok(pixel.iter().all(|value| value.is_finite()).then_some(pixel))
}

/// The full window renders UI while the CAD cameras/picker use its inner
/// logical canvas. Winit supplies the OS scale factor, not a webview estimate.
pub(crate) fn apply_interface_viewport(
    world: &mut World,
    rect: limo_cad_interface::Rect,
    scale_factor: f32,
) -> Result<(), String> {
    if [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .any(|v| !v.is_finite())
        || rect.x < 0.0
        || rect.y < 0.0
        || rect.width <= 0.0
        || rect.height <= 0.0
        || !scale_factor.is_finite()
        || scale_factor <= 0.0
    {
        return Err("Native CAD canvas needs finite positive bounds and scale".into());
    }
    let viewport = bevy::camera::Viewport {
        physical_position: UVec2::new(
            (rect.x * f64::from(scale_factor)).round() as u32,
            (rect.y * f64::from(scale_factor)).round() as u32,
        ),
        physical_size: UVec2::new(
            (rect.width * f64::from(scale_factor)).round().max(1.0) as u32,
            (rect.height * f64::from(scale_factor)).round().max(1.0) as u32,
        ),
        ..default()
    };
    for mut camera in world
        .query_filtered::<&mut Camera, With<NativeViewportCamera>>()
        .iter_mut(world)
    {
        if !camera.viewport.as_ref().is_some_and(|current| {
            current.physical_position == viewport.physical_position
                && current.physical_size == viewport.physical_size
        }) {
            camera.viewport = Some(viewport.clone());
        }
    }
    world
        .resource_mut::<ViewportSizeResource>()
        .set_if_neq(ViewportSizeResource {
            logical_width: rect.width as f32,
            logical_height: rect.height as f32,
        });
    let picker = world.resource::<SharedPickState>().0.clone();
    let mut picker = picker.lock().map_err(|_| "Native picker lock poisoned")?;
    picker.logical_size = (rect.width as f32, rect.height as f32);
    configure_viewport_line_widths(
        &mut world.resource_mut::<GizmoConfigStore>(),
        rect.width as f32,
        rect.height as f32,
        scale_factor,
    );
    Ok(())
}

fn apply_camera_state(world: &mut World, camera: ViewportCamera) {
    let mut resource = world.resource_mut::<CameraResource>();
    resource.camera = camera;
    resource.revision = resource.revision.wrapping_add(1);

    invalidate_interface_presentation(world);
}

fn apply_presentation_state(world: &mut World, next: ViewportPresentation) -> bool {
    let mut model = world.resource_mut::<ModelResource>();
    if (!Arc::ptr_eq(&model.body_poses, &next.body_poses) && model.body_poses != next.body_poses)
        || (!Arc::ptr_eq(&model.instance_body_poses, &next.instance_body_poses)
            && model.instance_body_poses != next.instance_body_poses)
    {
        let session_id = model.session_id.clone();
        model.bind_instance_state(
            &session_id,
            &next.instance_body_poses,
            InstanceUpdate::Presentation,
        );
        model.body_poses = next.body_poses.clone();
        model.instance_body_poses = next.instance_body_poses.clone();
        model.revision = model.revision.wrapping_add(1);
    }
    let mut resource = world.resource_mut::<PresentationResource>();
    if resource.0 == next {
        false
    } else {
        resource.0 = next;

        invalidate_interface_presentation(world);
        true
    }
}

/// Called synchronously inside the native reducer's document-owner guard.
/// The existing renderer and picker are updated together; no unowned view
/// command can arrive after a tab switch and affect its replacement.
pub(crate) fn apply_interface_view(
    world: &mut World,
    session_id: &str,
    camera: Option<ViewportCamera>,
    presentation: Option<ViewportPresentation>,
) -> Result<(), String> {
    if let Some(camera) = camera {
        validate_camera(camera)?;
    }
    if world.resource::<ModelResource>().session_id != session_id {
        return Err("Native viewport has not bound the requested document".into());
    }
    let picker = world
        .get_resource::<SharedPickState>()
        .ok_or("Native picker is unavailable")?
        .0
        .clone();
    let mut picker = picker.lock().map_err(|_| "Native picker lock poisoned")?;
    if let Some(camera) = camera {
        picker.camera = camera;
        apply_camera_state(world, camera);
    }
    if let Some(presentation) = presentation {
        picker.hidden_body_ids = presentation.hidden_body_ids.clone();
        picker.body_poses = presentation.body_poses.clone();
        picker.instance_body_poses = presentation.instance_body_poses.clone();
        apply_presentation_state(world, presentation);
    }
    Ok(())
}

fn validate_camera(camera: ViewportCamera) -> Result<(), String> {
    if camera
        .position
        .iter()
        .chain(camera.target.iter())
        .chain(camera.up.iter())
        .any(|v| !v.is_finite())
        || !camera.vertical_fov_degrees.is_finite()
        || camera.vertical_fov_degrees <= 0.0
        || camera.vertical_fov_degrees >= 180.0
    {
        return Err("Native camera must be finite with a valid field of view".into());
    }
    let direction = Vec3::from_array(camera.target) - Vec3::from_array(camera.position);
    let forward = direction.try_normalize();
    let up = Vec3::from_array(camera.up).try_normalize();
    if !direction.length().is_finite()
        || direction.length() > f32::MAX / 3.0
        || !forward
            .zip(up)
            .is_some_and(|(forward, up)| forward.cross(up).length_squared() > 1.0e-8)
    {
        return Err(
            "Native camera needs a finite nonzero view direction and independent up axis".into(),
        );
    }
    Ok(())
}

fn preview_mesh_content_changed(current: &ViewportPreview, next: &ViewportPreview) -> bool {
    current.triangles.len() != next.triangles.len()
        || current.triangles.iter().zip(&next.triangles).any(|(a, b)| {
            a.color != b.color
                || a.material != b.material
                || a.xray != b.xray
                || (!Arc::ptr_eq(&a.positions, &b.positions) && a.positions != b.positions)
                || (!Arc::ptr_eq(&a.normals, &b.normals) && a.normals != b.normals)
        })
        || current.arrows != next.arrows
}

/// A successful native File close retires this exact document's cached
/// entities and their strong asset handles. Other windows and warm tabs retain
/// their geometry; the File controller must not pass its whole tab inventory.
pub(crate) fn retire_interface_model_session(world: &mut World, session_id: &str) {
    drop_cached_model_session(world, session_id);
}

fn drop_cached_model_session(world: &mut World, session_id: &str) {
    let entities = {
        let mut query = world.query::<(Entity, &NativeModelGeometry)>();
        query
            .iter(world)
            .filter_map(|(entity, geometry)| (geometry.session_id == session_id).then_some(entity))
            .collect::<Vec<_>>()
    };
    for entity in entities {
        world.despawn(entity);
    }
    if let Some(entity) = world
        .get_resource_mut::<DocumentGeometryIndex>()
        .and_then(|mut index| index.0.remove(session_id))
    {
        world.despawn(entity);
    }
    world
        .resource_mut::<ModelResource>()
        .instance_states
        .remove(session_id);
}

struct CameraProjectionBasis {
    origin: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    tangent: f32,
    aspect: f32,
}

fn camera_projection(
    camera: ViewportCamera,
    viewport: (f32, f32),
) -> Option<CameraProjectionBasis> {
    if validate_camera(camera).is_err()
        || ![viewport.0, viewport.1]
            .iter()
            .all(|value| value.is_finite())
        || viewport.0 <= 1.0
        || viewport.1 <= 1.0
    {
        return None;
    }
    let origin = Vec3::from_array(camera.position);
    let forward = (Vec3::from_array(camera.target) - origin).normalize_or_zero();
    let up_hint = Vec3::from_array(camera.up).normalize_or_zero();
    let right = forward.cross(up_hint).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    if forward == Vec3::ZERO || right == Vec3::ZERO || up == Vec3::ZERO {
        return None;
    }
    Some(CameraProjectionBasis {
        origin,
        forward,
        right,
        up,
        tangent: (camera.vertical_fov_degrees.to_radians() * 0.5).tan(),
        aspect: viewport.0 / viewport.1,
    })
}

/// Geometry selection, world projection and sketch interaction share one basis.
fn camera_pick_ray(
    camera: ViewportCamera,
    viewport: (f32, f32),
    x: f32,
    y: f32,
) -> Option<(Vec3, Vec3, f32)> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let CameraProjectionBasis {
        origin,
        forward,
        right,
        up,
        tangent,
        aspect,
    } = camera_projection(camera, viewport)?;
    let ndc_x = x / viewport.0 * 2.0 - 1.0;
    let ndc_y = 1.0 - y / viewport.1 * 2.0;
    let direction = (forward + right * ndc_x * tangent * aspect + up * ndc_y * tangent).normalize();
    let world_per_pixel_factor = 2.0 * tangent / viewport.1;
    direction
        .is_finite()
        .then_some((origin, direction, world_per_pixel_factor))
}

mod edge_picking;
#[path = "physical_pick.rs"]
pub(crate) mod physical_pick;

fn pick_occt_scene(
    scene: &SolidSceneDto,
    (camera, viewport, x, y): (ViewportCamera, (f32, f32), f32, f32),
    hidden_body_ids: &[u64],
    body_poses: &[BodyPoseDto],
    instance_body_poses: &[InstanceBodyPoseDto],
    purpose: NativePickPurpose,
) -> Option<NativePick> {
    if matches!(
        purpose,
        NativePickPurpose::RefinableEdge
            | NativePickPurpose::Edge
            | NativePickPurpose::StraightEdge
            | NativePickPurpose::Vertex
    ) {
        return edge_picking::pick_edges(
            scene,
            (camera, viewport, [x, y]),
            hidden_body_ids,
            body_poses,
            instance_body_poses,
            purpose,
        );
    }
    let (origin, direction, world_per_pixel_factor) = camera_pick_ray(camera, viewport, x, y)?;
    let ray = (origin.as_dvec3(), direction.as_dvec3().normalize());
    let mut best: Option<NativePick> = None;
    for body in &scene.bodies {
        if hidden_body_ids.contains(&body.id.0) {
            continue;
        }
        let mut pick = |occurrence_id, transform| {
            pick_body(
                body,
                occurrence_id,
                transform,
                ray,
                world_per_pixel_factor,
                &mut best,
                purpose,
            );
        };
        if instance_body_poses.is_empty() {
            pick(None, body_pose_transform(body_poses, body.id.0));
        } else {
            for instance in instance_body_poses
                .iter()
                .filter(|instance| instance.body_id == body.id && instance.visible)
            {
                pick(
                    Some(instance.occurrence_id.0),
                    instance_body_pose_transform(
                        instance_body_poses,
                        body_poses,
                        body.id.0,
                        Some(instance.occurrence_id.0),
                    ),
                );
            }
        }
    }
    best
}

fn pick_body(
    body: &BodyDto,
    occurrence_id: Option<u64>,
    transform: Transform,
    (ray_origin, ray_direction): (bevy::math::DVec3, bevy::math::DVec3),
    world_per_pixel_factor: f32,
    best: &mut Option<NativePick>,
    purpose: NativePickPurpose,
) {
    let inverse_rotation = transform.rotation.inverse();
    for face in &body.faces {
        let start = face.first_index as usize;
        let end = start
            .saturating_add(face.index_count as usize)
            .min(body.mesh.indices.len());
        for triangle in body.mesh.indices[start..end].as_chunks::<3>().0 {
            let Some(a) =
                mesh_position(body, triangle[0]).map(|point| transform.transform_point(point))
            else {
                continue;
            };
            let Some(b) =
                mesh_position(body, triangle[1]).map(|point| transform.transform_point(point))
            else {
                continue;
            };
            let Some(c) =
                mesh_position(body, triangle[2]).map(|point| transform.transform_point(point))
            else {
                continue;
            };
            let Some(distance) = ray_triangle(
                ray_origin,
                ray_direction,
                a.as_dvec3(),
                b.as_dvec3(),
                c.as_dvec3(),
            ) else {
                continue;
            };
            let world_point = (ray_origin + ray_direction * distance).as_vec3();
            let local_point = inverse_rotation * (world_point - transform.translation);
            let connector = connector_for_face(face, local_point);
            if !pick_should_replace(
                best.as_ref(),
                distance,
                connector.as_ref().map(|value| value.kind),
            ) {
                continue;
            }
            *best = Some(NativePick {
                body_id: body.id.0,
                occurrence_id,
                face_id: face.id.0,
                edge_id: None,
                point: world_point.to_array(),
                distance,
                connector_kind: connector.as_ref().map(|value| value.kind.to_string()),
                connector_origin: connector.as_ref().map(|value| value.origin.to_array()),
                connector_primary_axis: connector
                    .as_ref()
                    .map(|value| value.primary_axis.to_array()),
                connector_secondary_axis: connector
                    .as_ref()
                    .map(|value| value.secondary_axis.to_array()),
                connector_radius: connector.as_ref().and_then(|value| value.radius),
            });
        }

        if purpose != NativePickPurpose::JointConnector {
            continue;
        }
        let Some(cylinder) = face.cylinder else {
            continue;
        };
        let axis = Vec3::new(
            cylinder.axis.x as f32,
            cylinder.axis.y as f32,
            cylinder.axis.z as f32,
        )
        .normalize_or_zero();
        if axis == Vec3::ZERO || !cylinder.radius.is_finite() || cylinder.radius <= 0.0 {
            continue;
        }
        let axis_origin = Vec3::new(
            cylinder.origin.x as f32,
            cylinder.origin.y as f32,
            cylinder.origin.z as f32,
        );
        let mut min_axial = f32::INFINITY;
        let mut max_axial = f32::NEG_INFINITY;
        for index in &body.mesh.indices[start..end] {
            let Some(point) = mesh_position(body, *index) else {
                continue;
            };
            let axial = (point - axis_origin).dot(axis);
            min_axial = min_axial.min(axial);
            max_axial = max_axial.max(axial);
        }
        if !min_axial.is_finite() || !max_axial.is_finite() || max_axial - min_axial <= 1.0e-5 {
            continue;
        }
        let reference = Vec3::new(
            cylinder.reference.x as f32,
            cylinder.reference.y as f32,
            cylinder.reference.z as f32,
        );
        for (axial, sign) in [(min_axial, -1.0_f32), (max_axial, 1.0_f32)] {
            let local_center = axis_origin + axis * axial;
            let local_primary = axis * sign;
            let world_center = transform.transform_point(local_center);
            let world_normal = (transform.rotation * local_primary).normalize_or_zero();
            let Some(distance) = ray_plane_disk(
                ray_origin,
                ray_direction,
                world_center.as_dvec3(),
                world_normal.as_dvec3(),
                cylinder.radius,
            ) else {
                continue;
            };
            if !pick_should_replace(best.as_ref(), distance, Some("virtual_circular_face")) {
                continue;
            }
            let secondary = orthogonal_reference(axis, reference);
            *best = Some(NativePick {
                body_id: body.id.0,
                occurrence_id,
                face_id: face.id.0,
                edge_id: None,
                point: (ray_origin + ray_direction * distance).as_vec3().to_array(),
                distance,
                connector_kind: Some("virtual_circular_face".to_string()),
                connector_origin: Some(local_center.to_array()),
                connector_primary_axis: Some(local_primary.to_array()),
                connector_secondary_axis: Some(secondary.to_array()),
                connector_radius: Some(cylinder.radius as f32),
            });
        }
    }

    if purpose != NativePickPurpose::JointConnector {
        return;
    }
    for edge in &body.edges {
        let Some(circle) = edge.circle.filter(|circle| circle.closed) else {
            continue;
        };
        if !circle.radius.is_finite() || circle.radius <= 0.0 {
            continue;
        }
        let local_center = Vec3::new(
            circle.center.x as f32,
            circle.center.y as f32,
            circle.center.z as f32,
        );
        let local_normal = Vec3::new(
            circle.normal.x as f32,
            circle.normal.y as f32,
            circle.normal.z as f32,
        )
        .normalize_or_zero();
        let local_reference = Vec3::new(
            circle.reference.x as f32,
            circle.reference.y as f32,
            circle.reference.z as f32,
        );
        if local_normal == Vec3::ZERO {
            continue;
        }
        let world_center = transform.transform_point(local_center);
        let world_normal = (transform.rotation * local_normal).normalize_or_zero();
        let Some(distance) = ray_plane_ring(
            ray_origin,
            ray_direction,
            world_center.as_dvec3(),
            world_normal.as_dvec3(),
            circle.radius,
            f64::from(world_per_pixel_factor),
        ) else {
            continue;
        };
        if !pick_should_replace(best.as_ref(), distance, Some("circular_edge")) {
            continue;
        }
        *best = Some(NativePick {
            body_id: body.id.0,
            occurrence_id,
            face_id: 0,
            edge_id: Some(edge.id.0),
            point: (ray_origin + ray_direction * distance).as_vec3().to_array(),
            distance,
            connector_kind: Some("circular_edge".to_string()),
            connector_origin: Some(local_center.to_array()),
            connector_primary_axis: Some(local_normal.to_array()),
            connector_secondary_axis: Some(
                orthogonal_reference(local_normal, local_reference).to_array(),
            ),
            connector_radius: Some(circle.radius as f32),
        });
    }
}

struct PickConnectorFrame {
    kind: &'static str,
    origin: Vec3,
    primary_axis: Vec3,
    secondary_axis: Vec3,
    radius: Option<f32>,
}

fn connector_for_face(
    face: &limo_cad_solid::FaceDto,
    local_point: Vec3,
) -> Option<PickConnectorFrame> {
    if let Some(plane) = face.plane {
        let origin = face.signature.map_or_else(
            || {
                Vec3::new(
                    plane.origin[0] as f32,
                    plane.origin[1] as f32,
                    plane.origin[2] as f32,
                )
            },
            |signature| {
                Vec3::new(
                    signature.centroid.x as f32,
                    signature.centroid.y as f32,
                    signature.centroid.z as f32,
                )
            },
        );
        return Some(PickConnectorFrame {
            kind: "planar_face",
            origin,
            primary_axis: Vec3::new(
                plane.normal[0] as f32,
                plane.normal[1] as f32,
                plane.normal[2] as f32,
            )
            .normalize_or_zero(),
            secondary_axis: Vec3::new(plane.u[0] as f32, plane.u[1] as f32, plane.u[2] as f32)
                .normalize_or_zero(),
            radius: None,
        });
    }
    let cylinder = face.cylinder?;
    let axis_origin = Vec3::new(
        cylinder.origin.x as f32,
        cylinder.origin.y as f32,
        cylinder.origin.z as f32,
    );
    let axis = Vec3::new(
        cylinder.axis.x as f32,
        cylinder.axis.y as f32,
        cylinder.axis.z as f32,
    )
    .normalize_or_zero();
    if axis == Vec3::ZERO {
        return None;
    }
    let origin = axis_origin + axis * (local_point - axis_origin).dot(axis);
    let reference = Vec3::new(
        cylinder.reference.x as f32,
        cylinder.reference.y as f32,
        cylinder.reference.z as f32,
    );
    let radial = local_point - origin;
    Some(PickConnectorFrame {
        kind: "cylindrical_face",
        origin,
        primary_axis: axis,
        secondary_axis: if radial.length_squared() > 1.0e-10 {
            radial.normalize()
        } else {
            orthogonal_reference(axis, reference)
        },
        radius: Some(cylinder.radius as f32),
    })
}

fn orthogonal_reference(axis: Vec3, candidate: Vec3) -> Vec3 {
    let projected = candidate - axis * candidate.dot(axis);
    if projected.length_squared() > 1.0e-10 {
        projected.normalize()
    } else {
        let fallback = if axis.cross(Vec3::X).length_squared() > 1.0e-10 {
            Vec3::X
        } else {
            Vec3::Y
        };
        (fallback - axis * fallback.dot(axis)).normalize_or_zero()
    }
}

fn pick_should_replace(
    current: Option<&NativePick>,
    distance: f64,
    candidate_kind: Option<&str>,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    const TIE_EPSILON: f64 = 1.0e-4;
    if distance < current.distance - TIE_EPSILON {
        return true;
    }
    if (distance - current.distance).abs() > TIE_EPSILON {
        return false;
    }
    let priority = |kind: Option<&str>| match kind {
        Some("circular_edge") => 2,
        Some("virtual_circular_face") => 0,
        _ => 1,
    };
    priority(candidate_kind) > priority(current.connector_kind.as_deref())
}

fn ray_plane_disk(
    origin: bevy::math::DVec3,
    direction: bevy::math::DVec3,
    center: bevy::math::DVec3,
    normal: bevy::math::DVec3,
    radius: f64,
) -> Option<f64> {
    let denominator = direction.dot(normal);
    if denominator.abs() <= 1.0e-7 {
        return None;
    }
    let distance = (center - origin).dot(normal) / denominator;
    if distance <= 1.0e-5 {
        return None;
    }
    let point = origin + direction * distance;
    ((point - center).length_squared() <= (radius * 1.025).powi(2)).then_some(distance)
}

fn ray_plane_ring(
    origin: bevy::math::DVec3,
    direction: bevy::math::DVec3,
    center: bevy::math::DVec3,
    normal: bevy::math::DVec3,
    radius: f64,
    world_per_pixel_factor: f64,
) -> Option<f64> {
    let denominator = direction.dot(normal);
    if denominator.abs() <= 1.0e-7 {
        return None;
    }
    let distance = (center - origin).dot(normal) / denominator;
    if distance <= 1.0e-5 {
        return None;
    }
    let point = origin + direction * distance;
    let radial_distance = (point - center).length();
    let tolerance = (distance * world_per_pixel_factor * 6.0)
        .max(radius * 0.01)
        .max(0.025);
    ((radial_distance - radius).abs() <= tolerance).then_some(distance)
}

fn mesh_position(body: &BodyDto, index: u32) -> Option<Vec3> {
    let offset = index as usize * 3;
    let coordinates = body.mesh.positions.get(offset..offset + 3)?;
    Some(Vec3::new(coordinates[0], coordinates[1], coordinates[2]))
}

fn ray_triangle(
    origin: bevy::math::DVec3,
    direction: bevy::math::DVec3,
    a: bevy::math::DVec3,
    b: bevy::math::DVec3,
    c: bevy::math::DVec3,
) -> Option<f64> {
    let edge_1 = b - a;
    let edge_2 = c - a;
    let p = direction.cross(edge_2);
    let determinant = edge_1.dot(p);
    if determinant.abs() < 1.0e-7 {
        return None;
    }
    let inverse = 1.0 / determinant;
    let t = origin - a;
    let u = t.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(edge_1);
    let v = direction.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = edge_2.dot(q) * inverse;
    (distance > 0.0).then_some(distance)
}

/// Application settings apply without changing document state or an unchanged resource.
pub(crate) fn apply_interface_gpu_stock_preference(world: &mut World, enabled: bool) {
    if world
        .get_resource::<PresentationResource>()
        .is_some_and(|state| state.0.cam_gpu_stock_removal != enabled)
    {
        world
            .resource_mut::<PresentationResource>()
            .0
            .cam_gpu_stock_removal = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_projection_round_trips_the_actual_pick_camera_and_rejects_retired_owners() {
        let mut app = interface_scene_fixture();
        app.world_mut().resource_mut::<ModelResource>().session_id = "owned".into();
        let viewport = (1280.0, 720.0);
        *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
            logical_width: viewport.0,
            logical_height: viewport.1,
        };
        for fov in [0.5_f32, 45.0, 170.0] {
            let camera = ViewportCamera {
                vertical_fov_degrees: fov,
                ..default()
            };
            app.world_mut().resource_mut::<CameraResource>().camera = camera;
            for pixel in [[0.0, 0.0], [640.0, 360.0], [1250.0, 690.0]] {
                let (origin, direction, _) =
                    camera_pick_ray(camera, viewport, pixel[0], pixel[1]).unwrap();
                let point = (origin.as_dvec3() + direction.as_dvec3() * 300.0).to_array();
                let actual = interface_world_point(app.world(), "owned", point)
                    .unwrap()
                    .unwrap();
                assert!(
                    (actual[0] - pixel[0]).abs() < 0.03,
                    "{actual:?} != {pixel:?}"
                );
                assert!(
                    (actual[1] - pixel[1]).abs() < 0.03,
                    "{actual:?} != {pixel:?}"
                );
                let behind = (origin.as_dvec3() - direction.as_dvec3() * 300.0).to_array();
                assert!(interface_world_point(app.world(), "owned", behind)
                    .unwrap()
                    .is_none());
            }
        }
        assert!(interface_world_point(app.world(), "retired", [0.0; 3]).is_err());
        assert!(interface_world_point(app.world(), "owned", [f64::NAN; 3]).is_err());
    }

    #[test]
    fn live_camera_projection_matches_the_accepted_view_at_small_and_large_scales() {
        for (distance, fov) in [(0.001_f32, 0.5_f32), (500., 15.2), (1_000_000., 170.)] {
            let mut app = App::new();
            let camera = ViewportCamera {
                position: [distance, 0., 0.],
                target: [0.; 3],
                up: [0., 0., 1.],
                vertical_fov_degrees: fov,
            };
            validate_camera(camera).unwrap();
            app.insert_resource(CameraResource {
                camera,
                revision: 1,
            })
            .init_resource::<PresentationResource>()
            .init_resource::<RenderedRevisions>()
            .insert_resource(GlobalAmbientLight::default())
            .add_systems(Update, apply_camera);
            let entity = app
                .world_mut()
                .spawn((
                    NativeViewportCamera,
                    Transform::default(),
                    Projection::Perspective(PerspectiveProjection::default()),
                ))
                .id();
            app.update();
            let Projection::Perspective(projection) =
                app.world().get::<Projection>(entity).unwrap()
            else {
                panic!("wrong projection")
            };
            assert!((projection.fov - fov.to_radians()).abs() < f32::EPSILON);
            assert!(projection.near < distance * 0.1);
            assert!(projection.far > distance * 2.0);
        }
        assert!(validate_camera(ViewportCamera {
            position: [0.; 3],
            target: [0.; 3],
            ..ViewportCamera::default()
        })
        .is_err());
        assert!(validate_camera(ViewportCamera {
            vertical_fov_degrees: f32::INFINITY,
            ..ViewportCamera::default()
        })
        .is_err());
    }

    #[test]
    fn hole_point_markers_sit_on_the_support_face() {
        let xy_at = |z: f64| PlaneBasis {
            origin: [0.0, 0.0, z],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        };
        let base_sketch = xy_at(0.0);
        let top_face = xy_at(15.0);
        let point = SketchVec2 { x: 3.0, y: -2.0 };
        let marker = hole_point_marker_world(&base_sketch, &point, Some(&top_face));
        assert!(
            (marker - Vec3::new(3.0, -2.0, 15.05)).length() < 1e-4,
            "{marker:?}"
        );
        let unsupported = hole_point_marker_world(&base_sketch, &point, None);
        assert!(
            (unsupported - Vec3::new(3.0, -2.0, 0.05)).length() < 1e-4,
            "{unsupported:?}"
        );
    }

    #[test]
    fn viewport_cameras_use_portable_msaa() {
        let mut gizmo_config = GizmoConfigStore::default();
        gizmo_config.insert(GizmoConfig::default(), CadHighlightGizmos);
        gizmo_config.insert(GizmoConfig::default(), CadModelEdgeGizmos);
        gizmo_config.insert(GizmoConfig::default(), CamCompletedPathGizmos);
        gizmo_config.insert(GizmoConfig::default(), CadSketchGizmos);
        gizmo_config.insert(GizmoConfig::default(), CadPickFeedbackGizmos);
        gizmo_config.insert(GizmoConfig::default(), CadDirectPickFeedbackGizmos);
        gizmo_config.insert(GizmoConfig::default(), CadProfileBorderGizmos);
        gizmo_config.insert(GizmoConfig::default(), CadPickFeedbackHaloGizmos);
        gizmo_config.insert(GizmoConfig::default(), CadSketchPointOutlineGizmos);
        gizmo_config.insert(GizmoConfig::default(), CadSketchPointGizmos);

        let mut app = App::new();
        app.insert_resource(gizmo_config)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<ReferencePlaneMaterial>>()
            .add_systems(Startup, setup_scene);
        app.update();

        let world = app.world_mut();
        let configs = world.resource::<GizmoConfigStore>();
        assert_eq!(
            configs.config::<CadModelEdgeGizmos>().0.depth_bias,
            0.0,
            "model edges use only bounded world-space lift, never relative depth bias"
        );
        let completed = configs.config::<CamCompletedPathGizmos>().0.depth_bias;
        assert!(
            -1.0 < completed && completed < 0.0,
            "traveled paths stay visible without a near-plane tie"
        );
        let mut cameras = world.query_filtered::<&Msaa, With<NativeViewportCamera>>();
        let sample_counts: Vec<Msaa> = cameras.iter(world).copied().collect();
        assert_eq!(
            sample_counts,
            vec![Msaa::Sample4; 2],
            "both CAD and overlay cameras must use portable 4x multisampling"
        );
    }

    #[test]
    fn model_edge_lift_preserves_thin_plate_occlusion_at_all_fixture_zooms() {
        let radius = Vec3::new(3., 3., 0.005).length();
        let lift = MODEL_EDGE_MAX_LIFT_MM.max(radius * MODEL_EDGE_MAX_LIFT_BODY_FRACTION);
        let plate_separation = 0.14 * 1.3_f32.sin();
        for zoom in [0.7, 1.0, 2.5] {
            let distance = 212. * zoom;
            let near = (distance / 100_000_f32).max(0.001);
            let lifted_edge_depth = near / (distance - lift);
            let plate_depth = near / (distance - plate_separation);
            assert!(
                lifted_edge_depth < plate_depth,
                "bounded stroke must remain behind the plate at zoom {zoom}"
            );
            let old_biased_depth =
                lifted_edge_depth * ((distance - lift) / near - 4.88e-4).powf(0.0001);
            assert!(
                old_biased_depth > plate_depth,
                "fixture must reproduce the old relative-bias leak at zoom {zoom}"
            );
        }
    }

    #[test]
    fn shared_native_edge_eligibility_distinguishes_straight_from_bent_edges() {
        let edge = |points: &[[f64; 3]]| limo_cad_solid::EdgeDto {
            id: limo_cad_core::EdgeId(1),
            key: "edge".to_string(),
            points: points
                .iter()
                .map(|point| limo_cad_solid::Point3Dto {
                    x: point[0],
                    y: point[1],
                    z: point[2],
                })
                .collect(),
            circle: None,
            refinable: true,
        };
        assert!(edge_is_straight(&edge(&[
            [0.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
        ])));
        assert!(!edge_is_straight(&edge(&[
            [0.0, 0.0, 0.0],
            [5.0, 1.0, 0.0],
            [10.0, 0.0, 0.0],
        ])));
    }

    fn assert_engine_ok(response: String) -> serde_json::Value {
        let envelope: serde_json::Value =
            serde_json::from_str(&response).expect("engine response should be JSON");
        assert_eq!(envelope["ok"], true, "engine error: {envelope}");
        envelope["value"].clone()
    }

    #[test]
    fn cad_studio_lights_are_camera_relative_and_reveal_different_face_normals() {
        let camera = ViewportCamera::default();
        let target = Vec3::from_array(camera.target);
        let view = (Vec3::from_array(camera.position) - target).normalize();
        let up = Vec3::from_array(camera.up).normalize();
        let right = view.cross(up).normalize();
        let (key, fill) = camera_relative_light_transforms(camera);
        let key_direction = (key.translation - target).normalize();
        let fill_direction = (fill.translation - target).normalize();

        assert!(key_direction.dot(view) > 0. && fill_direction.dot(view) > 0.);
        assert!(key_direction.dot(right) > 0. && fill_direction.dot(right) < 0.);
        assert!(key_direction.dot(up) > fill_direction.dot(up));

        let rotated = ViewportCamera {
            position: [-170.0, -170.0, 130.0],
            ..camera
        };
        let (rotated_key, rotated_fill) = camera_relative_light_transforms(rotated);
        assert_ne!(key.translation, rotated_key.translation);
        assert_ne!(fill.translation, rotated_fill.translation);
    }

    #[test]
    fn camera_transform_stays_finite_at_up_axis_alignment() {
        for position in [[0.0, 0.0, -25.0], [0.0, 0.0, 25.0]] {
            let transform = camera_transform(ViewportCamera {
                position,
                target: [0.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
                ..ViewportCamera::default()
            });
            assert!(
                transform
                    .to_matrix()
                    .to_cols_array()
                    .iter()
                    .all(|value| value.is_finite()),
                "camera transform must remain finite at position {position:?}"
            );
        }
    }

    #[test]
    fn retained_cam_tool_places_flute_and_shank_from_the_tip_axis() {
        let geometry = limo_cad_cam::CamCutterGeometryDto {
            kind: limo_cad_cam::CamToolKind::Drill,
            diameter: 6.,
            flute_length: 20.,
            overall_length: 50.,
            point_angle_degrees: Some(118.),
            corner_radius: None,
            corner_chamfer: None,
        };
        let tool = ViewportCamTool {
            tip: [1., 2., 3.],
            axis: [1., 0., 0.],
            geometry,
        };
        let flute = cam_tool_part_transform(tool, NativeCamToolPartKind::Flute).unwrap();
        let shank = cam_tool_part_transform(tool, NativeCamToolPartKind::Shank).unwrap();
        assert_eq!(flute, shank);
        assert!(flute.translation.abs_diff_eq(Vec3::new(1., 2., 3.), 1e-6));
        assert!(flute.scale.abs_diff_eq(Vec3::ONE, 1e-6));
        assert!((flute.rotation * Vec3::Z).abs_diff_eq(Vec3::X, 1e-6));
        let source = limo_cad_cam::cutter_mesh(geometry).unwrap();
        let mesh = cam_cutter_mesh(&source.cutter);
        assert_eq!(mesh.count_vertices(), source.cutter.positions.len() / 3);
        assert!(mesh.contains_attribute(Mesh::ATTRIBUTE_NORMAL));

        let moved = ViewportCamTool {
            tip: [3., 4., 5.],
            ..tool
        };
        assert_eq!(moved.geometry, tool.geometry);
    }

    #[test]
    fn retained_cam_cutter_reuses_gpu_assets_during_motion_and_tool_changes() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<StandardMaterial>>();
        let mut tool = ViewportCamTool {
            tip: [0.; 3],
            axis: [0., 0., 1.],
            geometry: limo_cad_cam::CamCutterGeometryDto {
                kind: limo_cad_cam::CamToolKind::Drill,
                diameter: 6.,
                flute_length: 20.,
                overall_length: 50.,
                point_angle_degrees: Some(118.),
                corner_radius: None,
                corner_chamfer: None,
            },
        };
        app.insert_resource(PresentationResource(ViewportPresentation {
            cam_tool: Some(tool),
            ..default()
        }));
        app.add_systems(Update, update_native_cam_tool);
        app.update();
        let handles = app
            .world()
            .resource::<Assets<Mesh>>()
            .ids()
            .collect::<Vec<_>>();
        assert_eq!(handles.len(), 2);
        for i in 0..120 {
            tool.tip = [i as f32, (i as f32 / 10.).sin(), -1.];
            if i == 60 {
                tool.geometry.kind = limo_cad_cam::CamToolKind::BullNoseEndMill;
                tool.geometry.point_angle_degrees = None;
                tool.geometry.corner_radius = Some(1.);
            }
            app.world_mut()
                .resource_mut::<PresentationResource>()
                .0
                .cam_tool = Some(tool);
            app.update();
            assert_eq!(
                app.world()
                    .resource::<Assets<Mesh>>()
                    .ids()
                    .collect::<Vec<_>>(),
                handles
            );
            assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 2);
        }
        tool.geometry.corner_radius = Some(f64::NAN);
        app.world_mut()
            .resource_mut::<PresentationResource>()
            .0
            .cam_tool = Some(tool);
        app.update();
        assert!(app
            .world_mut()
            .query_filtered::<&Visibility, With<NativeCamToolPart>>()
            .iter(app.world())
            .all(|v| *v == Visibility::Hidden));
    }

    #[test]
    fn native_model_carries_closed_profiles_from_an_internal_midplane_sketch() {
        let state = crate::state::AppState::new();
        assert_engine_ok(
            state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#),
        );
        assert_engine_ok(state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-20.0,"y":-20.0},
                "p2":{"x":20.0,"y":20.0},
                "ctrl_held":false
            }"#,
        ));
        assert_engine_ok(state.engine_call("end_sketch", ""));
        assert_engine_ok(state.solid_extrude(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "operation":"new_body",
                "extent":{"type":"distance","distance":20.0},
                "taper_angle_deg":0.0,
                "flip":false,
                "target_body_ids":[]
            }"#,
        ));
        assert_engine_ok(state.engine_call(
            "datum_plane_create",
            r#"{
                "source":{
                    "type":"offset",
                    "reference":{"type":"origin_plane","plane":"xy"},
                    "distance":20.0
                }
            }"#,
        ));
        assert_engine_ok(state.engine_call(
            "datum_plane_create",
            r#"{
                "source":{
                    "type":"midplane",
                    "first":{"type":"origin_plane","plane":"xy"},
                    "second":{"type":"datum_plane","datum_id":1}
                }
            }"#,
        ));
        assert_engine_ok(
            state.engine_call("begin_sketch", r#"{"type":"datum_plane","datum_id":2}"#),
        );
        assert_engine_ok(state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-5.0,"y":-5.0},
                "p2":{"x":5.0,"y":5.0},
                "ctrl_held":false
            }"#,
        ));
        assert_engine_ok(state.engine_call("end_sketch", ""));

        let (_, _, scene, _, _, datum_planes, profile_catalog, _, _, _) = state.viewport_snapshot();
        assert_eq!(scene.bodies.len(), 1);
        assert_eq!(datum_planes.len(), 2);
        assert!((datum_planes[1].basis.origin[2] - 10.0).abs() < 1.0e-9);
        let profile = profile_catalog
            .iter()
            .find(|entry| entry.sketch_name == "Sketch2")
            .expect("the internal midplane sketch must reach the native model");
        assert_eq!(profile.profiles.len(), 1);
        assert_eq!(profile.profiles[0].nesting_depth, 0);
        assert_eq!(profile.profiles[0].points.len(), 4);
        assert!((profile.basis.origin[2] - 10.0).abs() < 1.0e-9);
    }

    #[test]
    fn visible_sketch_points_use_the_persistent_always_on_top_layer() {
        let point: EntityDto = serde_json::from_str(
            r#"{
                "kind": "point",
                "id": 7,
                "position": { "x": 0.0, "y": 0.0 },
                "fully_defined": false
            }"#,
        )
        .expect("standalone sketch point should deserialize");

        assert_eq!(sketch_grip_positions(&point), &[SketchVec2::ZERO]);
        assert!((-1.0..=1.0).contains(&SKETCH_DEPTH_BIAS));
        const {
            assert!(SKETCH_POINT_OUTLINE_DEPTH_BIAS > SKETCH_DEPTH_BIAS);
        }
        const {
            assert!(SKETCH_LINE_WIDTH < HIGHLIGHT_LINE_WIDTH);
        }
        const {
            assert!(SKETCH_POINT_OUTLINE_WIDTH > SKETCH_LINE_WIDTH);
        }
        const {
            assert!(SKETCH_POINT_OUTLINE_WIDTH <= 2.0);
        }
        const {
            assert!(SKETCH_POINT_OUTLINE_RADIUS_PX > SKETCH_POINT_RADIUS_PX);
        }
        const {
            assert!(SKETCH_POINT_OUTLINE_RADIUS_PX < 3.5);
        }
        const {
            assert!(HIGHLIGHT_LINE_WIDTH <= 2.0);
        }
    }

    #[test]
    fn interaction_line_weight_tracks_logical_viewport_and_backing_density() {
        assert_eq!(DIRECT_PICK_FEEDBACK_OFFSET, PROFILE_PICK_OFFSET);
        assert_eq!(PICK_FEEDBACK_LINE_WIDTH, 1.0);
        assert_eq!(DIRECT_PICK_FEEDBACK_LINE_WIDTH, 0.75);
        assert_eq!(PROFILE_BORDER_LINE_WIDTH, DIRECT_PICK_FEEDBACK_LINE_WIDTH);
        const {
            assert!(PROFILE_BORDER_LINE_WIDTH < SKETCH_LINE_WIDTH);
        }
        const {
            assert!(DIRECT_PICK_FEEDBACK_LINE_WIDTH < PICK_FEEDBACK_LINE_WIDTH);
        }
        const {
            assert!(PICK_FEEDBACK_LINE_WIDTH < SKETCH_LINE_WIDTH);
        }
        const {
            assert!(HIGHLIGHT_LINE_WIDTH > PICK_FEEDBACK_LINE_WIDTH);
        }
        assert_eq!(
            PICK_FEEDBACK_HALO_LINE_WIDTH,
            PICK_FEEDBACK_LINE_WIDTH * 2.0
        );
        for bias in [
            SKETCH_DEPTH_BIAS,
            SKETCH_POINT_OUTLINE_DEPTH_BIAS,
            PICK_FEEDBACK_HALO_DEPTH_BIAS,
            PICK_FEEDBACK_DEPTH_BIAS,
            DIRECT_PICK_FEEDBACK_DEPTH_BIAS,
        ] {
            assert!(
                (-1.0..=1.0).contains(&bias),
                "gizmo depth bias {bias} must stay inside Bevy's documented range",
            );
        }
        const {
            assert!(SKETCH_DEPTH_BIAS > PICK_FEEDBACK_HALO_DEPTH_BIAS);
        }
        const {
            assert!(PICK_FEEDBACK_HALO_DEPTH_BIAS > PICK_FEEDBACK_DEPTH_BIAS);
        }
        const {
            assert!(PICK_FEEDBACK_DEPTH_BIAS > DIRECT_PICK_FEEDBACK_DEPTH_BIAS);
        }
        assert!((viewport_line_logical_scale(960.0, 720.0) - 1.0).abs() < 1.0e-6);
        assert!(viewport_line_logical_scale(1600.0, 1000.0) > 1.0);
        assert_eq!(
            viewport_line_logical_scale(10_000.0, 10_000.0),
            VIEWPORT_LINE_SCALE_MAX
        );
        assert_eq!(
            viewport_line_logical_scale(320.0, 240.0),
            VIEWPORT_LINE_SCALE_MIN
        );
        let logical = viewport_line_raster_scale(1600.0, 1000.0, 1.0);
        let retina = viewport_line_raster_scale(1600.0, 1000.0, 2.0);
        assert!((retina - logical * 2.0).abs() < 1.0e-6);
    }

    #[test]
    fn sketch_point_radius_stays_constant_in_screen_space_at_different_depths() {
        let camera = ViewportCamera {
            position: [0.0, 0.0, 10.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
        };
        let viewport = ViewportSizeResource {
            logical_width: 1_200.0,
            logical_height: 800.0,
        };
        let near = Vec3::ZERO;
        let far = Vec3::new(0.0, 0.0, -90.0);

        let near_radius = screen_space_disc_radius(camera, viewport, near, SKETCH_POINT_RADIUS_PX);
        let far_radius = screen_space_disc_radius(camera, viewport, far, SKETCH_POINT_RADIUS_PX);
        let near_world_per_pixel = world_per_pixel_at(camera, viewport, near);
        let far_world_per_pixel = world_per_pixel_at(camera, viewport, far);

        assert!((near_radius / near_world_per_pixel - SKETCH_POINT_RADIUS_PX).abs() < 1.0e-5);
        assert!((far_radius / far_world_per_pixel - SKETCH_POINT_RADIUS_PX).abs() < 1.0e-5);
        assert!(far_radius > near_radius);
    }

    #[test]
    fn sketch_point_billboard_axes_are_orthonormal() {
        let (right, up) = camera_facing_axes(ViewportCamera::default());

        assert!((right.length() - 1.0).abs() < 1.0e-5);
        assert!((up.length() - 1.0).abs() < 1.0e-5);
        assert!(right.dot(up).abs() < 1.0e-5);
    }

    #[test]
    fn native_preview_preserves_endpoint_snap_semantics() {
        let preview: ViewportPreview = serde_json::from_str(
            r#"{
                "lines": [{
                    "color": [0.2, 0.7, 1.0, 0.68],
                    "width": 1.15,
                    "pattern": "dotted",
                    "segments": [0.0, 0.0, 0.0, 10.0, 0.0, 0.0]
                }],
                "points": [],
                "triangles": [{
                    "color": [1.0, 0.4, 0.2, 0.25],
                    "positions": [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 5.0, 0.0],
                    "normals": [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
                    "material": "machined_stock",
                    "xray": true
                }],
                "arrows": [{
                    "start": [0.0, 0.0, 0.0],
                    "end": [0.0, 0.0, 10.0],
                    "color": [0.2, 0.7, 1.0, 1.0],
                    "width": 2.0,
                    "xray": true
                }],
                "annotations": [],
                "marker": {
                    "position": [12.0, -4.0, 0.18],
                    "kind": "point"
                }
            }"#,
        )
        .expect("semantic snap marker should deserialize across the engine boundary");

        let marker = preview.marker.expect("endpoint marker should be retained");
        assert_eq!(marker.kind, ViewportSnapKind::Point);
        assert_eq!(marker.position, [12.0, -4.0, 0.18]);
        assert_eq!(preview.lines[0].pattern, ViewportLinePattern::Dotted);
        assert_eq!(preview.lines[0].segments.len(), 6);
        assert!(preview.triangles[0].xray);
        assert_eq!(preview.triangles[0].positions.len(), 9);
        assert_eq!(preview.triangles[0].normals.len(), 9);
        assert_eq!(
            preview.triangles[0].material,
            crate::native_viewport::ViewportTriangleMaterial::MachinedStock
        );
        assert_eq!(preview.arrows[0].end, [0.0, 0.0, 10.0]);
        assert_eq!(preview.arrows[0].width, 2.0);
        const {
            assert!(HIGHLIGHT_LINE_WIDTH <= 2.0);
        }
        const {
            assert!(SNAP_MARKER_HALF_SIZE_PX >= 5.0);
        }
    }

    #[test]
    fn retained_preview_reads_and_restores_share_buffers_without_changing_bevy_ticks() {
        let mut app = interface_scene_fixture();
        app.world_mut().resource_mut::<ModelResource>().session_id = "preview-owner".into();
        let preview: ViewportPreview = serde_json::from_value(serde_json::json!({
            "lines": [{"segments": [0., 0., 0., 1., 0., 0.], "playback": {
                "pathId": 1, "completedColor": [1., 1., 1., 1.], "segmentTimes": [0., 1.]
            }}],
            "points": [{"positions": [0., 0., 0.]}],
            "triangles": [{"positions": [0., 0., 0., 1., 0., 0., 0., 1., 0.],
                "normals": [0., 0., 1., 0., 0., 1., 0., 0., 1.]}],
            "annotations": [{"screen": [10., 20.], "text": "retained"}]
        }))
        .unwrap();
        apply_interface_preview(app.world_mut(), "preview-owner", preview).unwrap();
        let original = interface_preview_snapshot(app.world());
        let revision = interface_preview_revision(app.world());
        let mesh_revision = app.world().resource::<PreviewResource>().mesh_revision;
        app.world_mut().clear_trackers();
        let read = interface_preview_snapshot(app.world());
        assert!(Arc::ptr_eq(&original, &read));
        apply_interface_preview(app.world_mut(), "preview-owner", read).unwrap();
        assert_eq!(interface_preview_revision(app.world()), revision);
        assert!(!app
            .world()
            .get_resource_ref::<PreviewResource>()
            .unwrap()
            .is_changed());
        assert!(apply_interface_preview(app.world_mut(), "stale-owner", original.clone()).is_err());

        let mut overlay = original.as_ref().clone();
        overlay.annotations[0].screen = [40., 50.];
        assert!(Arc::ptr_eq(
            &original.lines[0].segments,
            &overlay.lines[0].segments
        ));
        assert!(Arc::ptr_eq(
            &original.lines[0].playback.as_ref().unwrap().segment_times,
            &overlay.lines[0].playback.as_ref().unwrap().segment_times
        ));
        assert!(Arc::ptr_eq(
            &original.points[0].positions,
            &overlay.points[0].positions
        ));
        assert!(Arc::ptr_eq(
            &original.triangles[0].positions,
            &overlay.triangles[0].positions
        ));
        assert!(Arc::ptr_eq(
            &original.triangles[0].normals,
            &overlay.triangles[0].normals
        ));
        apply_interface_preview(app.world_mut(), "preview-owner", overlay).unwrap();
        assert_eq!(
            app.world().resource::<PreviewResource>().mesh_revision,
            mesh_revision
        );

        let mut edited = original.as_ref().clone();
        Arc::make_mut(&mut edited.triangles[0].positions)[0] = 5.;
        assert_eq!(original.triangles[0].positions[0], 0.);
        assert!(!Arc::ptr_eq(
            &original.triangles[0].positions,
            &edited.triangles[0].positions
        ));
        apply_interface_preview(app.world_mut(), "preview-owner", edited).unwrap();
        assert_eq!(
            app.world().resource::<PreviewResource>().mesh_revision,
            mesh_revision + 1
        );
        apply_interface_preview(app.world_mut(), "preview-owner", original.clone()).unwrap();
        assert!(Arc::ptr_eq(
            &original,
            &interface_preview_snapshot(app.world())
        ));
    }

    #[test]
    fn camera_frequency_annotation_updates_do_not_rebuild_preview_meshes() {
        let base: ViewportPreview = serde_json::from_str(
            r#"{
                "lines": [],
                "points": [],
                "triangles": [{
                    "color": [0.2, 0.7, 1.0, 0.25],
                    "positions": [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0],
                    "xray": true
                }],
                "arrows": [{
                    "start": [0.0, 0.0, 0.0],
                    "end": [0.0, 0.0, 10.0],
                    "color": [0.2, 0.7, 1.0, 1.0],
                    "width": 2.0,
                    "xray": true
                }],
                "annotations": [{
                    "screen": [100.0, 100.0],
                    "color": [1.0, 1.0, 1.0, 1.0],
                    "text": "10 mm",
                    "kind": "dimension"
                }],
                "marker": null
            }"#,
        )
        .expect("preview should deserialize");
        assert_eq!(
            base.triangles[0].material,
            crate::native_viewport::ViewportTriangleMaterial::Overlay
        );
        let mut moved_annotation = base.clone();
        moved_annotation.annotations[0].screen = [420.0, 240.0];
        assert!(
            !preview_mesh_content_changed(&base, &moved_annotation),
            "camera projection updates must keep retained GPU meshes"
        );

        let mut edited_tool = base.clone();
        edited_tool.arrows[0].end[2] = 25.0;
        assert!(preview_mesh_content_changed(&base, &edited_tool));
    }

    #[test]
    fn inside_corner_faces_lift_the_stroke_and_outside_corners_do_not() {
        let along = Vec3::Z;
        let middle = Vec3::ZERO;
        let forward = Vec3::new(-0.6, 0.6, -0.5).normalize();
        let half_width = 1.5;

        let inside = [
            Some(EdgeSideFace {
                normal: Vec3::NEG_Y,
                interior: Vec3::new(2.5, 0.0, 2.5),
            }),
            Some(EdgeSideFace {
                normal: Vec3::X,
                interior: Vec3::new(0.0, -10.0, 2.5),
            }),
        ];
        let rise = edge_stroke_rise_px(along, middle, forward, &inside, half_width);
        assert!(
            rise > half_width * 0.5 && rise.is_finite(),
            "inside corner rise {rise}"
        );

        let outside = [
            Some(EdgeSideFace {
                normal: Vec3::NEG_Y,
                interior: Vec3::new(-10.0, 0.0, 2.5),
            }),
            Some(EdgeSideFace {
                normal: Vec3::X,
                interior: Vec3::new(0.0, 10.0, 2.5),
            }),
        ];
        assert_eq!(
            edge_stroke_rise_px(along, middle, forward, &outside, half_width),
            0.0
        );

        assert_eq!(
            edge_stroke_rise_px(along, middle, forward, &[None, None], half_width),
            0.0
        );
    }

    #[test]
    fn edge_side_faces_follow_boundary_membership() {
        let plane = |normal: [f64; 3]| limo_cad_core::PlaneBasis {
            origin: [0.0; 3],
            u: [1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
            normal,
        };
        let signature =
            |centroid: [f64; 3], normal: [f64; 3]| limo_cad_solid::PlanarFaceSignatureDto {
                centroid: limo_cad_solid::Point3Dto {
                    x: centroid[0],
                    y: centroid[1],
                    z: centroid[2],
                },
                normal: limo_cad_solid::Point3Dto {
                    x: normal[0],
                    y: normal[1],
                    z: normal[2],
                },
                area: 1.0,
                perimeter: 4.0,
                wire_count: 1,
                edge_count: 4,
            };
        let face = |id: u64, normal: [f64; 3], centroid: [f64; 3], edges: &[&str]| FaceDto {
            linear_seam_edge_keys: Vec::new(),
            outer_shell: None,
            id: limo_cad_core::FaceId(id),
            key: format!("face:{id}"),
            first_index: 0,
            index_count: 0,
            plane: Some(plane(normal)),
            signature: Some(signature(centroid, normal)),
            cylinder: None,
            edge_keys: edges.iter().map(|edge| edge.to_string()).collect(),
            cone: None,
        };
        let body = BodyDto {
            id: limo_cad_core::BodyId(1),
            topology_signature: String::new(),
            display_warnings: Vec::new(),
            name: "Plate".into(),
            feature_id: limo_cad_core::FeatureId(2),
            mesh: limo_cad_solid::MeshDto {
                positions: Vec::new(),
                normals: Vec::new(),
                indices: Vec::new(),
            },
            faces: vec![
                face(
                    1,
                    [0.0, -1.0, 0.0],
                    [22.5, 20.0, 2.5],
                    &["edge:4", "edge:9"],
                ),
                face(2, [1.0, 0.0, 0.0], [20.0, 10.0, 2.5], &["edge:4", "edge:7"]),
            ],
            edges: Vec::new(),
        };
        let sides = edge_side_faces(&body, &Transform::IDENTITY);
        let corner = sides["edge:4"];
        assert_eq!(corner[0].map(|side| side.normal), Some(Vec3::NEG_Y));
        assert_eq!(corner[1].map(|side| side.normal), Some(Vec3::X));
        assert_eq!(sides["edge:9"][1], None);
        assert!(!sides.contains_key("edge:1"));

        let transform = Transform::from_translation(Vec3::new(3., 5., 7.))
            .with_rotation(Quat::from_rotation_z(0.7))
            .with_scale(Vec3::splat(2.));
        let transformed = edge_side_faces(&body, &transform);
        for (key, sides) in sides {
            assert_eq!(transform_edge_sides(sides, &transform), transformed[key]);
        }

        let mut model = ModelResource {
            session_id: "a".into(),
            geometry_revision: 1,
            document: std::sync::Arc::new(limo_cad_native_engine::NativeViewportDocument {
                scene: std::sync::Arc::new(SolidSceneDto {
                    bodies: vec![body],
                    errors: vec![],
                }),
                ..Default::default()
            }),
            ..default()
        };
        let mut cache = ModelEdgeCache::default();
        cache.update(&model.document.scene);
        assert!(cache.bodies.contains_key(&1));
        model.session_id = "b".into();
        Arc::make_mut(&mut Arc::make_mut(&mut model.document).scene).bodies[0].id =
            limo_cad_core::BodyId(2);
        cache.update(&model.document.scene);
        assert!(!cache.bodies.contains_key(&1));
        assert!(cache.bodies.contains_key(&2));
        model.instance_revision += 1;
        Arc::make_mut(&mut Arc::make_mut(&mut model.document).scene).bodies[0].id =
            limo_cad_core::BodyId(3);
        cache.update(&model.document.scene);
        assert!(!cache.bodies.contains_key(&2));
        assert!(cache.bodies.contains_key(&3));
        model.geometry_revision += 1;
        Arc::make_mut(&mut Arc::make_mut(&mut model.document).scene)
            .bodies
            .clear();
        cache.update(&model.document.scene);
        assert!(cache.bodies.is_empty());
    }

    #[test]
    fn grid_step_follows_the_1_2_5_sequence_and_clamps() {
        let close = |actual: f32, expected: f32| (actual - expected).abs() <= expected * 1.0e-5;

        assert!(close(adaptive_grid_step(0.1), 2.0));
        assert!(close(adaptive_grid_step(0.3), 10.0));
        assert!(close(adaptive_grid_step(1.0), 20.0));
        assert!(close(adaptive_grid_step(2.0), 50.0));
        assert!(close(adaptive_grid_step(1.0e-9), GRID_MIN_STEP));
        assert!(close(adaptive_grid_step(1.0e9), GRID_MAX_STEP));
        assert!(close(adaptive_grid_step(f32::NAN), 10.0));
    }

    #[test]
    fn grid_lattices_nest_along_the_sequence() {
        assert_eq!(one_two_five_ceiling(0.3), 0.5);
        assert_eq!(one_two_five_ceiling(2.0), 2.0);
        assert_eq!(one_two_five_ceiling(2.1), 5.0);
        assert_eq!(one_two_five_ceiling(60.0), 100.0);
        assert_eq!(one_two_five_ceiling(-1.0), GRID_MIN_STEP);

        assert_eq!(coarsest_lattice_ratio(3, 2), 1.0);
        assert_eq!(coarsest_lattice_ratio(5, 2), 5.0);
        assert_eq!(coarsest_lattice_ratio(25, 2), 25.0);
        assert_eq!(coarsest_lattice_ratio(-25, 2), 25.0);
        assert!(coarsest_lattice_ratio(0, 2).is_infinite());

        assert_eq!(coarsest_lattice_ratio(2, 5), 2.0);
        assert_eq!(coarsest_lattice_ratio(3, 5), 1.0);
        assert_eq!(coarsest_lattice_ratio(20, 5), 20.0);
    }

    #[test]
    fn grid_lines_brighten_continuously_with_their_spacing() {
        let fine = Color::srgba(0.2, 0.2, 0.2, 0.3);
        let major = Color::srgba(0.4, 0.4, 0.4, 0.5);
        let alpha = |spacing: f32| grid_line_color(spacing, fine, major).alpha();
        assert_eq!(alpha(4.0), 0.0);
        assert!((alpha(20.0) - 0.3).abs() < 1.0e-6);
        assert!((alpha(300.0) - 0.5).abs() < 1.0e-6);
        let mut previous = alpha(1.0);
        let mut spacing = 1.0f32;
        while spacing < 1000.0 {
            spacing *= 1.02;
            let next = alpha(spacing);
            assert!(
                next >= previous - 1.0e-6,
                "brightness never drops as a lattice spreads"
            );
            assert!(next - previous < 0.02, "no visible jump at {spacing} px");
            previous = next;
        }
    }

    #[test]
    fn grid_sheet_follows_zoom_and_pan_without_jumps() {
        let viewport = ViewportSizeResource {
            logical_width: 1_200.0,
            logical_height: 800.0,
        };
        let [(_, ground, _), ..] = origin_plane_bases();
        let fine = rgba([0.2, 0.2, 0.2], 0.28);
        let major = rgba([0.4, 0.4, 0.4], 0.48);

        let mut previous: Option<f32> = None;
        let mut distance = 40.0f32;
        while distance < 4_000.0 {
            let camera = ViewportCamera {
                position: [0.0, 0.0, distance],
                target: [0.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                vertical_fov_degrees: 45.0,
            };
            let layout = grid_layout(camera, viewport, &ground);
            assert!([1, 2, 5].contains(&layout.finest_mantissa));
            let pixel = world_per_pixel_at(camera, viewport, Vec3::ZERO);
            assert!(
                layout.finest / pixel >= GRID_LINE_FADE_IN_PX[0] - 1.0e-3,
                "the finest lattice is never drawn denser than its fade-in spacing"
            );
            assert!(
                layout.finest / pixel < GRID_LINE_FADE_IN_PX[0] * 2.5 + 1.0e-3,
                "the finest lattice is never coarser than the next member down would allow"
            );
            assert!((layout.radius - pixel * 800.0 * GRID_SHEET_RADIUS_HEIGHTS).abs() < 1.0e-3);

            let index = (20.0 / layout.finest).round() as i64;
            let alpha = if (index as f32 * layout.finest - 20.0).abs() < 1.0e-4 {
                let spacing = coarsest_lattice_ratio(index, layout.finest_mantissa) as f32
                    * layout.finest
                    / pixel;
                grid_line_color(spacing, fine, major).alpha()
            } else {
                0.0
            };
            if let Some(previous) = previous {
                assert!(
                    (alpha - previous).abs() < 0.02,
                    "20 mm line alpha jumped {previous} -> {alpha} at distance {distance}"
                );
            }
            previous = Some(alpha);
            distance *= 1.01;
        }

        let panned = ViewportCamera {
            position: [12_345.6, -678.9, 200.0],
            target: [12_345.6, -678.9, 0.0],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
        };
        let layout = grid_layout(panned, viewport, &ground);
        assert!((layout.center.x - 12_345.6).abs() < 1.0e-2);
        assert!((layout.center.y + 678.9).abs() < 1.0e-2);
    }

    #[test]
    fn reference_planes_scale_with_camera_depth() {
        let viewport = ViewportSizeResource {
            logical_width: 1_200.0,
            logical_height: 800.0,
        };
        let near = ViewportCamera {
            position: [0.0, 0.0, 100.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
        };
        let far = ViewportCamera {
            position: [0.0, 0.0, 200.0],
            ..near
        };
        let near_half = reference_plane_half_size(near, viewport, Vec3::ZERO);
        let far_half = reference_plane_half_size(far, viewport, Vec3::ZERO);
        assert!(near_half > 0.0);
        assert!((far_half / near_half - 2.0).abs() < 1.0e-5);
    }

    #[test]
    fn ray_triangle_returns_forward_hit() {
        let distance = ray_triangle(
            bevy::math::DVec3::new(0.0, 0.0, 5.0),
            bevy::math::DVec3::NEG_Z,
            bevy::math::DVec3::new(-1.0, -1.0, 0.0),
            bevy::math::DVec3::new(1.0, -1.0, 0.0),
            bevy::math::DVec3::new(0.0, 1.0, 0.0),
        )
        .expect("ray should hit");
        assert!((distance - 5.0).abs() < 1.0e-5);
    }

    #[test]
    fn native_reference_plane_matches_the_react_pick_footprint() {
        let mesh = reference_plane_mesh(&origin_plane_bases()[0].1, REFERENCE_PLANE_HALF_SIZE);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|values| values.as_float3())
            .expect("reference plane should expose float3 positions");
        let min_x = positions
            .iter()
            .map(|position| position[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = positions
            .iter()
            .map(|position| position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = positions
            .iter()
            .map(|position| position[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = positions
            .iter()
            .map(|position| position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(max_x - min_x, 100.0);
        assert_eq!(max_y - min_y, 100.0);
    }

    #[test]
    fn support_picking_uses_finite_visible_quads_and_forward_rays() {
        let xy = limo_cad_core::PlaneRef::ORIGIN_PLANES[0]
            .origin_basis()
            .unwrap();
        assert_eq!(
            ray_reference_quad(Vec3::new(3., 4., 20.), Vec3::NEG_Z, xy, 10.),
            Some(20.)
        );
        assert!(ray_reference_quad(Vec3::new(11., 4., 20.), Vec3::NEG_Z, xy, 10.).is_none());
        assert!(ray_reference_quad(Vec3::new(3., 4., 20.), Vec3::X, xy, 10.).is_none());
        assert!(ray_reference_quad(Vec3::new(3., 4., 20.), Vec3::Z, xy, 10.).is_none());
        assert!(ray_reference_quad(Vec3::NAN, Vec3::NEG_Z, xy, 10.).is_none());
        assert!(
            ray_reference_quad(Vec3::new(3., 4., 20.), Vec3::NEG_Z, xy, f32::INFINITY).is_none()
        );
    }

    #[test]
    fn highlighted_face_boundary_omits_shared_tessellation_diagonal() {
        let positions = vec![
            0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 0.0, 0.0, 10.0, 10.0, 0.0, 0.0,
            10.0, 0.0,
        ];
        let segments = triangle_boundary_segments(&positions, &[0, 1, 2, 3, 4, 5]);
        assert_eq!(segments.len(), 4, "only the quad perimeter should remain");
        let first = Vec3::ZERO;
        let opposite = Vec3::new(10.0, 10.0, 0.0);
        assert!(
            !segments.iter().any(|(start, end)| {
                (*start == first && *end == opposite) || (*start == opposite && *end == first)
            }),
            "the internal triangulation diagonal must not be rendered"
        );
    }

    #[test]
    fn native_highlight_stroke_respects_two_pixel_cap() {
        const {
            assert!(HIGHLIGHT_LINE_WIDTH <= 2.0);
        }
    }

    #[test]
    fn native_picker_hits_an_actual_occt_extrusion_snapshot() {
        let state = crate::state::AppState::new();
        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-10.0,"y":-10.0},
                "p2":{"x":10.0,"y":10.0},
                "ctrl_held":false
            }"#,
        );
        state.engine_call("end_sketch", "");
        state.solid_extrude(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "operation":"new_body",
                "extent":{"type":"distance","distance":10.0},
                "taper_angle_deg":0.0,
                "flip":false,
                "target_body_ids":[]
            }"#,
        );

        let (_, _, scene, _, _, _, _, _, _, _) = state.viewport_snapshot();
        assert_eq!(scene.bodies.len(), 1);
        assert_eq!(scene.bodies[0].mesh.indices.len(), 36);
        let hit = pick_occt_scene(
            &scene,
            (
                ViewportCamera {
                    position: [0.0, 0.0, 100.0],
                    target: [0.0, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                    vertical_fov_degrees: 45.0,
                },
                (800.0, 600.0),
                400.0,
                300.0,
            ),
            &[],
            &[],
            &[],
            NativePickPurpose::Geometry,
        )
        .expect("center ray should hit the OCCT box");
        assert_eq!(hit.body_id, scene.bodies[0].id.0);
        assert!(hit.point[2] > 9.99);
        assert!(
            pick_occt_scene(
                &scene,
                (
                    ViewportCamera {
                        position: [0.0, 0.0, 100.0],
                        target: [0.0, 0.0, 0.0],
                        up: [0.0, 1.0, 0.0],
                        vertical_fov_degrees: 45.0,
                    },
                    (800.0, 600.0),
                    400.0,
                    300.0
                ),
                &[scene.bodies[0].id.0],
                &[],
                &[],
                NativePickPurpose::Geometry
            )
            .is_none(),
            "browser-hidden bodies must not remain pickable"
        );
        let translated = BodyPoseDto {
            body_id: scene.bodies[0].id,
            translation: [40.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        let moved_hit = pick_occt_scene(
            &scene,
            (
                ViewportCamera {
                    position: [40.0, 0.0, 100.0],
                    target: [40.0, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                    vertical_fov_degrees: 45.0,
                },
                (800.0, 600.0),
                400.0,
                300.0,
            ),
            &[],
            &[translated],
            &[],
            NativePickPurpose::Geometry,
        )
        .expect("native picking must follow the solved body pose");
        assert!((moved_hit.point[0] - 40.0).abs() < 1.0e-4);
        let mesh = body_mesh(&scene.bodies[0])
            .expect("committed OCCT geometry should become one indexed Bevy mesh per body");
        assert_eq!(
            mesh.count_vertices(),
            scene.bodies[0].mesh.positions.len() / 3
        );
        assert_eq!(
            mesh.indices().expect("body mesh should stay indexed").len(),
            scene.bodies[0].mesh.indices.len(),
            "body batching must preserve the complete OCCT tessellation"
        );
        let overlay = face_mesh(&scene.bodies[0], &scene.bodies[0].faces[0])
            .expect("a hovered or selected face can still create a transient overlay");
        assert_eq!(
            overlay.count_vertices(),
            scene.bodies[0].faces[0].index_count as usize
        );

        let expected_face_count = scene.bodies[0].faces.len();
        let mut render_app = App::new();
        render_app
            .init_resource::<ModelResource>()
            .init_resource::<RenderedRevisions>()
            .init_resource::<PaletteResource>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, rebuild_occt_meshes);
        {
            let mut model = render_app.world_mut().resource_mut::<ModelResource>();
            model.session_id = "batched-body-test".to_string();
            model.geometry_revision = 1;
            Arc::make_mut(&mut model.document).scene = Arc::new(scene.clone());
            model.revision = 1;
        }
        bind_document_geometry(render_app.world_mut());
        render_app.update();
        let body_draws = {
            let world = render_app.world_mut();
            let mut query = world.query::<(&NativeCadBody, &Mesh3d)>();
            query.iter(world).count()
        };
        let face_metadata = {
            let world = render_app.world_mut();
            let mut query = world.query::<(&NativeCadFace, Option<&Mesh3d>)>();
            query
                .iter(world)
                .inspect(|(_, mesh)| assert!(mesh.is_none()))
                .count()
        };
        assert_eq!(
            body_draws, 1,
            "one solid body should use one committed draw mesh"
        );
        assert_eq!(face_metadata, expected_face_count);

        let started = Instant::now();
        for _ in 0..10_000 {
            std::hint::black_box(pick_occt_scene(
                &scene,
                (
                    ViewportCamera {
                        position: [0.0, 0.0, 100.0],
                        target: [0.0, 0.0, 0.0],
                        up: [0.0, 1.0, 0.0],
                        vertical_fov_degrees: 45.0,
                    },
                    (800.0, 600.0),
                    400.0,
                    300.0,
                ),
                &[],
                &[],
                &[],
                NativePickPurpose::Geometry,
            ));
        }
        let average_micros = started.elapsed().as_secs_f64() * 100.0;
        eprintln!("actual OCCT box pick average: {average_micros:.3} Âµs");
        assert!(
            average_micros < 5_000.0,
            "native picking exceeded the demo's 5 ms CPU budget"
        );
    }

    #[test]
    fn native_occurrences_share_meshes_and_pick_the_exact_instance() {
        let state = crate::state::AppState::new();
        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-10.0,"y":-10.0},
                "p2":{"x":10.0,"y":10.0},
                "ctrl_held":false
            }"#,
        );
        state.engine_call("end_sketch", "");
        state.solid_extrude(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "operation":"new_body",
                "extent":{"type":"distance","distance":10.0},
                "taper_angle_deg":0.0,
                "flip":false,
                "target_body_ids":[]
            }"#,
        );
        let (_, _, scene, _, _, _, _, _, _, _) = state.viewport_snapshot();
        let body_id = scene.bodies[0].id;
        let instances = vec![
            InstanceBodyPoseDto {
                occurrence_id: limo_cad_sketch::OccurrenceId(41),
                component_id: limo_cad_sketch::ComponentId(7),
                body_id,
                translation: [-30.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                visible: true,
            },
            InstanceBodyPoseDto {
                occurrence_id: limo_cad_sketch::OccurrenceId(42),
                component_id: limo_cad_sketch::ComponentId(7),
                body_id,
                translation: [30.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                visible: true,
            },
        ];
        for (x, occurrence_id) in [(-30.0, 41), (30.0, 42)] {
            let hit = pick_occt_scene(
                &scene,
                (
                    ViewportCamera {
                        position: [x, 0.0, 100.0],
                        target: [x, 0.0, 0.0],
                        up: [0.0, 1.0, 0.0],
                        vertical_fov_degrees: 45.0,
                    },
                    (800.0, 600.0),
                    400.0,
                    300.0,
                ),
                &[],
                &[],
                &instances,
                NativePickPurpose::Geometry,
            )
            .expect("each visible occurrence should be independently pickable");
            assert_eq!(hit.body_id, body_id.0);
            assert_eq!(hit.occurrence_id, Some(occurrence_id));
        }

        let mut render_app = App::new();
        render_app
            .init_resource::<ModelResource>()
            .init_resource::<RenderedRevisions>()
            .init_resource::<PaletteResource>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, rebuild_occt_meshes);
        {
            let mut model = render_app.world_mut().resource_mut::<ModelResource>();
            model.session_id = "instance-mesh-test".to_string();
            model.geometry_revision = 1;
            model.instance_revision = 1;
            Arc::make_mut(&mut model.document).scene = Arc::new(scene);
            model.instance_body_poses = instances.into();
            model.revision = 1;
        }
        bind_document_geometry(render_app.world_mut());
        render_app.update();
        let mesh_ids = {
            let world = render_app.world_mut();
            let mut query = world.query::<(&NativeCadBody, &Mesh3d)>();
            query
                .iter(world)
                .map(|(body, mesh)| (body.occurrence_id, mesh.0.id()))
                .collect::<Vec<_>>()
        };
        assert_eq!(mesh_ids.len(), 2);
        assert_ne!(mesh_ids[0].0, mesh_ids[1].0);
        assert_eq!(
            mesh_ids[0].1, mesh_ids[1].1,
            "reusable occurrences must retain one shared GPU mesh asset",
        );
    }

    #[test]
    fn native_pick_purpose_defaults_to_geometry_and_requires_explicit_connector_opt_in() {
        assert_eq!(NativePickPurpose::default(), NativePickPurpose::Geometry);
        assert_eq!(
            serde_json::from_str::<NativePickPurpose>(r#""geometry""#).unwrap(),
            NativePickPurpose::Geometry,
        );
        assert_eq!(
            serde_json::from_str::<NativePickPurpose>(r#""jointConnector""#).unwrap(),
            NativePickPurpose::JointConnector,
        );
        assert!(serde_json::from_str::<NativePickPurpose>(r#""unknown""#).is_err());
    }

    #[test]
    fn native_geometry_picker_ignores_virtual_caps_on_an_actual_partial_revolve() {
        let state = crate::state::AppState::new();
        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xz"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":2.0,"y":0.0},
                "p2":{"x":5.0,"y":10.0},
                "ctrl_held":true
            }"#,
        );
        state.engine_call("end_sketch", "");
        let response = state.solid_revolve(
            r#"{
                "sketch_name":"Sketch1",
                "profile_indices":[0],
                "axis_origin":{"x":0.0,"y":0.0},
                "axis_direction":{"x":0.0,"y":1.0},
                "angle_deg":80.0,
                "flip":false,
                "operation":"new_body",
                "target_body_ids":[]
            }"#,
        );
        let (_, _, scene, _, _, _, _, _, _, _) = state.viewport_snapshot();
        assert_eq!(scene.bodies.len(), 1, "{response}");
        assert!(scene.errors.is_empty());
        let body = &scene.bodies[0];
        assert!(body.faces.iter().any(|face| face.cylinder.is_some()));

        let empty_sector_camera = ViewportCamera {
            position: [-3.0, 0.0, 50.0],
            target: [-3.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            vertical_fov_degrees: 45.0,
        };
        let pick = |scene: &SolidSceneDto, camera, purpose| {
            pick_occt_scene(
                scene,
                (camera, (800.0, 600.0), 400.0, 300.0),
                &[],
                &[],
                &[],
                purpose,
            )
        };
        assert!(
            pick(&scene, empty_sector_camera, NativePickPurpose::Geometry).is_none(),
            "empty space beside a partial revolve must not select its cylindrical wall",
        );
        let connector = pick(
            &scene,
            empty_sector_camera,
            NativePickPurpose::JointConnector,
        )
        .expect("explicit joint picking retains the virtual opening target");
        assert_eq!(
            connector.connector_kind.as_deref(),
            Some("virtual_circular_face")
        );

        let top = body
            .faces
            .iter()
            .find(|face| {
                face.plane
                    .as_ref()
                    .is_some_and(|plane| plane.normal[2].abs() > 0.99 && plane.origin[2] > 9.99)
            })
            .expect("partial revolve has a physical top cap");
        let triangle = &body.mesh.indices[top.first_index as usize..top.first_index as usize + 3];
        let center = triangle
            .iter()
            .map(|index| mesh_position(body, *index).unwrap())
            .sum::<Vec3>()
            / 3.0;
        let cap_camera = ViewportCamera {
            position: (center + Vec3::Z * 50.0).to_array(),
            target: center.to_array(),
            ..empty_sector_camera
        };
        for purpose in [
            NativePickPurpose::Geometry,
            NativePickPurpose::JointConnector,
        ] {
            let cap = pick(&scene, cap_camera, purpose).expect("physical cap remains selectable");
            assert_eq!(cap.face_id, top.id.0, "{purpose:?} at {center:?}: {cap:?}");
            assert_ne!(cap.connector_kind.as_deref(), Some("virtual_circular_face"));
            assert!((Vec3::from_array(cap.point) - center).length() < 1.0e-4);
        }

        state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#);
        state.engine_call(
            "add_rectangle",
            r#"{
                "mode":"two_point",
                "p1":{"x":-4.0,"y":-1.0},
                "p2":{"x":-2.0,"y":1.0},
                "ctrl_held":true
            }"#,
        );
        state.engine_call("end_sketch", "");
        let response = state.solid_extrude(
            r#"{
                "sketch_name":"Sketch2", "profile_indices":[0],
                "operation":"new_body", "extent":{"type":"distance","distance":5.0},
                "taper_angle_deg":0.0, "flip":true, "target_body_ids":[]
            }"#,
        );
        let (_, _, scene, _, _, _, _, _, _, _) = state.viewport_snapshot();
        assert_eq!(scene.bodies.len(), 2, "{response}");
        let behind = pick(&scene, empty_sector_camera, NativePickPurpose::Geometry)
            .expect("real geometry behind a virtual disk remains selectable");
        assert_eq!(behind.body_id, scene.bodies[1].id.0);
        assert!(behind.point[2].abs() < 1.0e-4);
    }

    #[test]
    fn native_picker_exposes_a_virtual_circular_connector_at_a_cylinder_opening() {
        let body = limo_cad_solid::BodyDto {
            topology_signature: String::new(),
            display_warnings: Vec::new(),
            id: limo_cad_core::BodyId(7),
            name: "Holed component".to_string(),
            feature_id: limo_cad_core::FeatureId(3),
            mesh: limo_cad_solid::MeshDto {
                positions: vec![5.0, 0.0, 0.0, 5.0, 0.0, 10.0, 0.0, 5.0, 0.0, 0.0, 5.0, 10.0],
                normals: vec![0.0; 12],
                indices: vec![0, 1, 2, 2, 1, 3],
            },
            faces: vec![limo_cad_solid::FaceDto {
                linear_seam_edge_keys: Vec::new(),
                outer_shell: None,
                id: limo_cad_core::FaceId(70),
                key: "cylindrical-wall".to_string(),
                edge_keys: vec![],
                cone: None,
                first_index: 0,
                index_count: 6,
                plane: None,
                signature: None,
                cylinder: Some(limo_cad_solid::CylindricalSurfaceDto {
                    origin: limo_cad_solid::Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    axis: limo_cad_solid::Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    reference: limo_cad_solid::Point3Dto {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    radius: 5.0,
                }),
            }],
            edges: vec![limo_cad_solid::EdgeDto {
                id: limo_cad_core::EdgeId(71),
                key: "outer-chamfer-rim".to_string(),
                points: vec![],
                circle: Some(limo_cad_solid::CircularCurveDto {
                    center: limo_cad_solid::Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 10.0,
                    },
                    normal: limo_cad_solid::Point3Dto {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                    reference: limo_cad_solid::Point3Dto {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    radius: 8.0,
                    closed: true,
                }),
                refinable: true,
            }],
        };
        let scene = SolidSceneDto {
            bodies: vec![body],
            errors: vec![],
        };
        for x in [0.0, 8.0] {
            assert!(
                pick_occt_scene(
                    &scene,
                    (
                        ViewportCamera {
                            position: [x, 0.0, 50.0],
                            target: [x, 0.0, 0.0],
                            up: [0.0, 1.0, 0.0],
                            vertical_fov_degrees: 45.0,
                        },
                        (800.0, 600.0),
                        400.0,
                        300.0
                    ),
                    &[],
                    &[],
                    &[],
                    NativePickPurpose::Geometry
                )
                .is_none(),
                "ordinary face picking must not hit virtual openings or connector rings at x={x}",
            );
        }
        let hit = pick_occt_scene(
            &scene,
            (
                ViewportCamera {
                    position: [0.0, 0.0, 50.0],
                    target: [0.0, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                    vertical_fov_degrees: 45.0,
                },
                (800.0, 600.0),
                400.0,
                300.0,
            ),
            &[],
            &[],
            &[],
            NativePickPurpose::JointConnector,
        )
        .expect("the otherwise empty cylinder opening should be pickable");
        assert_eq!(hit.connector_kind.as_deref(), Some("virtual_circular_face"));
        assert_eq!(hit.connector_origin, Some([0.0, 0.0, 10.0]));
        assert_eq!(hit.connector_primary_axis, Some([0.0, 0.0, 1.0]));

        let rim_hit = pick_occt_scene(
            &scene,
            (
                ViewportCamera {
                    position: [8.0, 0.0, 50.0],
                    target: [8.0, 0.0, 0.0],
                    up: [0.0, 1.0, 0.0],
                    vertical_fov_degrees: 45.0,
                },
                (800.0, 600.0),
                400.0,
                300.0,
            ),
            &[],
            &[],
            &[],
            NativePickPurpose::JointConnector,
        )
        .expect("the exact outer chamfer rim should be pickable");
        assert_eq!(rim_hit.connector_kind.as_deref(), Some("circular_edge"));
        assert_eq!(rim_hit.edge_id, Some(71));
        assert_eq!(rim_hit.connector_origin, Some([0.0, 0.0, 10.0]));
        assert_eq!(rim_hit.connector_primary_axis, Some([0.0, 0.0, 1.0]));
        assert_eq!(rim_hit.connector_radius, Some(8.0));

        let wall_hit = pick_occt_scene(
            &scene,
            (
                ViewportCamera {
                    position: [50.0, 0.0, 5.0],
                    target: [0.0, 0.0, 5.0],
                    up: [0.0, 0.0, 1.0],
                    vertical_fov_degrees: 45.0,
                },
                (800.0, 600.0),
                400.0,
                300.0,
            ),
            &[],
            &[],
            &[],
            NativePickPurpose::Geometry,
        )
        .expect("the physical internal cylinder wall should remain pickable");
        assert_eq!(wall_hit.connector_kind.as_deref(), Some("cylindrical_face"));
        assert_eq!(wall_hit.connector_origin, Some([0.0, 0.0, 5.0]));
        assert_eq!(wall_hit.connector_primary_axis, Some([0.0, 0.0, 1.0]));
        assert_eq!(wall_hit.connector_secondary_axis, Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn native_body_material_uses_the_document_appearance() {
        let body_id = 73;
        let mut appearance = BodyAppearance::default_for(limo_cad_core::BodyId(body_id));
        appearance.color = limo_cad_core::Rgba8::opaque(12, 123, 240);
        let model = ModelResource {
            document: std::sync::Arc::new(limo_cad_native_engine::NativeViewportDocument {
                body_appearances: vec![appearance],
                ..Default::default()
            }),
            ..default()
        };

        let color = body_appearance_color(&model, body_id, [0.1, 0.2, 0.3]).to_srgba();
        assert!((color.red - 12.0 / 255.0).abs() < 1.0e-6);
        assert!((color.green - 123.0 / 255.0).abs() < 1.0e-6);
        assert!((color.blue - 240.0 / 255.0).abs() < 1.0e-6);
    }

    #[test]
    fn isolated_edit_model_cannot_reuse_live_meshes_with_the_same_geometry_counter() {
        let mut app = interface_scene_fixture();
        let snapshot =
            crate::session_bridge::native_interface::model_snapshot(&crate::state::AppState::new());
        apply_interface_model(app.world_mut(), snapshot.clone()).unwrap();
        let before = app.world().resource::<ModelResource>().instance_revision;
        apply_interface_edit_model(app.world_mut(), snapshot.clone()).unwrap();
        let staged = app.world().resource::<ModelResource>().instance_revision;
        assert_ne!(staged, before);
        assert!(app.world().resource::<ModelResource>().transient_model);
        apply_interface_model(app.world_mut(), snapshot).unwrap();
        assert_ne!(
            app.world().resource::<ModelResource>().instance_revision,
            staged
        );
        assert!(!app.world().resource::<ModelResource>().transient_model);
    }

    #[test]
    fn engine_frames_share_geometry_with_renderer_picker_and_preserve_old_scenes() {
        let engine = crate::state::AppState::new();
        let before = engine.viewport_frame();
        assert!(Arc::ptr_eq(
            &before.document.scene,
            &engine.solid_scene_snapshot()
        ));
        for (operation, payload) in [
            ("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#),
            (
                "add_rectangle",
                r#"{"mode":"two_point","p1":{"x":0.0,"y":0.0},"p2":{"x":20.0,"y":10.0},"ctrl_held":false}"#,
            ),
            ("end_sketch", ""),
        ] {
            let result: serde_json::Value =
                serde_json::from_str(&engine.engine_call(operation, payload)).unwrap();
            assert_eq!(result["ok"], true, "{result}");
        }
        let result: serde_json::Value = serde_json::from_str(&engine.solid_extrude(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3.0},"taper_angle_deg":0.0,"flip":false,"target_body_ids":[]}"#)).unwrap();
        assert_eq!(result["ok"], true, "{result}");
        let frame = engine.viewport_frame();
        assert!(before.document.scene.bodies.is_empty());
        assert_eq!(frame.document.scene.bodies.len(), 1);
        assert!(!Arc::ptr_eq(&before.document.scene, &frame.document.scene));
        let scene = Arc::clone(&frame.document.scene);
        let poses = Arc::clone(&frame.body_poses);
        let instances = Arc::clone(&frame.instance_body_poses);
        let mut app = interface_scene_fixture();
        apply_interface_model(app.world_mut(), frame).unwrap();
        assert!(Arc::ptr_eq(
            &app.world().resource::<ModelResource>().document.scene,
            &scene
        ));
        {
            let picker = app.world().resource::<SharedPickState>().0.lock().unwrap();
            assert!(Arc::ptr_eq(&picker.scene, &scene));
            assert!(Arc::ptr_eq(&picker.body_poses, &poses));
            assert!(Arc::ptr_eq(&picker.instance_body_poses, &instances));
        }
        let model = app.world().resource::<ModelResource>();
        assert!(Arc::ptr_eq(&model.body_poses, &poses));
        assert!(Arc::ptr_eq(&model.instance_body_poses, &instances));
        crate::session_bridge::native_interface::refresh_native_model(
            &engine,
            app.world_mut(),
            false,
        )
        .unwrap();
        let (_, _, presentation, _) = interface_view(app.world());
        assert!(Arc::ptr_eq(&presentation.body_poses, &poses));
        assert!(Arc::ptr_eq(&presentation.instance_body_poses, &instances));
        assert!(Arc::ptr_eq(&engine.viewport_frame().document.scene, &scene));

        interface_geometry_fixture_snapshot(app.world_mut());
        let body_id = scene.bodies[0].id.0;
        let session_id = &app.world().resource::<ModelResource>().session_id;
        assert_eq!(
            interface_body_local_center(app.world(), session_id, &scene, body_id),
            Some([10., 5., 1.5])
        );
        assert_eq!(
            interface_body_local_center(app.world(), "other-document", &scene, body_id),
            None
        );
        assert_eq!(
            interface_body_local_center(
                app.world(),
                session_id,
                &Arc::new((*scene).clone()),
                body_id
            ),
            None,
            "Equal geometry from another snapshot cannot reuse cached bounds"
        );
        let face_id = scene.bodies[0].faces[0].id.0;
        app.world_mut().init_resource::<section_view::State>();
        app.world_mut()
            .resource_mut::<PresentationResource>()
            .0
            .selected_face_ids = vec![face_id];
        let system = app
            .world_mut()
            .register_system(rebuild_native_face_overlays);
        app.world_mut().run_system(system).unwrap();
        let highlights = |world: &mut World| {
            world
                .query_filtered::<&Mesh3d, With<NativeCadFaceOverlay>>()
                .iter(world)
                .map(|mesh| mesh.0.id())
                .collect::<Vec<_>>()
        };
        let original = highlights(app.world_mut());
        assert!(!original.is_empty());
        app.world_mut().clear_trackers();
        section_view::clear(app.world_mut());
        assert!(!app
            .world()
            .get_resource_ref::<section_view::State>()
            .unwrap()
            .is_changed());
        app.world_mut().run_system(system).unwrap();
        assert_eq!(
            highlights(app.world_mut()),
            original,
            "An idle section inspector must preserve selected face mesh handles"
        );
        {
            let mut state = app.world_mut().resource_mut::<PresentationResource>();
            state.0.hide_sketch_grid = !state.0.hide_sketch_grid;
            state.0.selected_sketch_entity_ids = (1..=1000).collect();
        }
        let tick = app
            .world()
            .get_resource_ref::<PresentationResource>()
            .unwrap()
            .last_changed();
        let (_, _, borrowed, _) = interface_view(app.world());
        assert!(std::ptr::eq(
            borrowed,
            &app.world().resource::<PresentationResource>().0
        ));
        assert_eq!(borrowed.selected_sketch_entity_ids.len(), 1000);
        assert_eq!(
            app.world()
                .get_resource_ref::<PresentationResource>()
                .unwrap()
                .last_changed(),
            tick
        );
        app.world_mut().run_system(system).unwrap();
        assert_eq!(
            highlights(app.world_mut()),
            original,
            "Unrelated presentation changes retain face highlight meshes"
        );
        app.world_mut()
            .resource_mut::<PresentationResource>()
            .0
            .selected_face_ids
            .clear();
        app.world_mut().run_system(system).unwrap();
        assert!(highlights(app.world_mut()).is_empty());
    }

    fn instance_cache_model(session_id: &str, occurrences: &[u64]) -> ViewportModel {
        ViewportModel {
            session_id: session_id.into(),
            geometry_revision: 1,
            body_poses: Arc::default(),
            instance_body_poses: Arc::new(
                occurrences
                    .iter()
                    .map(|id| InstanceBodyPoseDto {
                        occurrence_id: limo_cad_sketch::OccurrenceId(*id),
                        component_id: limo_cad_sketch::ComponentId(1),
                        body_id: limo_cad_core::BodyId(1),
                        translation: [0.; 3],
                        rotation: [0., 0., 0., 1.],
                        visible: true,
                    })
                    .collect(),
            ),
            document: std::sync::Arc::new(limo_cad_native_engine::NativeViewportDocument {
                scene: std::sync::Arc::new(SolidSceneDto {
                    bodies: vec![BodyDto {
                        id: limo_cad_core::BodyId(1),
                        topology_signature: String::new(),
                        display_warnings: Vec::new(),
                        name: "Cache triangle".into(),
                        feature_id: limo_cad_core::FeatureId(1),
                        mesh: limo_cad_solid::MeshDto {
                            positions: vec![0., 0., 0., 1., 0., 0., 0., 1., 0.],
                            normals: vec![0., 0., 1., 0., 0., 1., 0., 0., 1.],
                            indices: vec![0, 1, 2],
                        },
                        faces: vec![],
                        edges: vec![],
                    }],
                    errors: vec![],
                }),
                active_sketch: None,
                finished_sketches: vec![],
                datum_planes: vec![],
                profile_catalog: vec![],
                body_appearances: vec![],
                ..Default::default()
            }),
        }
    }

    #[test]
    fn component_edit_fades_sibling_instances_and_retains_their_shared_meshes() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = interface_scene_fixture();
        *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
            logical_width: 800.,
            logical_height: 600.,
        };
        let mut model = instance_cache_model("component-edit", &[1, 2]);
        let plane = limo_cad_core::PlaneRef::OriginPlane {
            plane: limo_cad_core::OriginPlane::Xy,
        };
        let mut sketch = limo_cad_sketch::SketchSession::new(
            "Shared",
            plane,
            plane.origin_basis().unwrap(),
            false,
        )
        .dto();
        sketch.edit_occurrence_id = Some(limo_cad_sketch::OccurrenceId(2));
        sketch.basis = PlaneBasis {
            origin: [50., 25., 10.],
            u: [0., 1., 0.],
            v: [-1., 0., 0.],
            normal: [0., 0., 1.],
        };
        let edit_basis = sketch.basis;
        let document = std::sync::Arc::make_mut(&mut model.document);
        document.metadata_revision = 1;
        document.active_sketch = Some(sketch);
        apply_interface_model(app.world_mut(), model.clone()).unwrap();
        let target = edit_basis.to_3d([7., 4.]).map(|value| value as f32);
        apply_interface_view(
            app.world_mut(),
            "component-edit",
            Some(ViewportCamera {
                position: [target[0], target[1], target[2] + 100.],
                target,
                up: [0., 1., 0.],
                vertical_fov_degrees: 45.,
            }),
            None,
        )
        .unwrap();
        let size = app.world().resource::<ViewportSizeResource>();
        let screen = [size.logical_width * 0.5, size.logical_height * 0.5];
        let picked = interface_sketch_point(app.world(), "component-edit", screen, edit_basis)
            .unwrap()
            .unwrap();
        assert!((picked.x - 7.).abs() < 1e-6 && (picked.y - 4.).abs() < 1e-6);
        let geometry = interface_geometry_fixture_snapshot(app.world_mut());
        app.world_mut()
            .run_system_once(apply_native_presentation_styles)
            .unwrap();
        let mut query = app
            .world_mut()
            .query::<(&NativeCadBody, &MeshMaterial3d<StandardMaterial>)>();
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let mut count = 0;
        for (body, handle) in query.iter(app.world()) {
            let material = materials.get(&handle.0).unwrap();
            assert_eq!(
                material.alpha_mode,
                if body.occurrence_id == Some(2) {
                    AlphaMode::Opaque
                } else {
                    AlphaMode::Blend
                }
            );
            count += 1;
        }
        assert_eq!(count, 2);
        let document = std::sync::Arc::make_mut(&mut model.document);
        document.metadata_revision = 2;
        document.active_sketch = None;
        apply_interface_model(app.world_mut(), model).unwrap();
        assert_eq!(
            interface_geometry_fixture_snapshot(app.world_mut()),
            geometry
        );
        app.world_mut()
            .run_system_once(apply_native_presentation_styles)
            .unwrap();
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        for (_, handle) in query.iter(app.world()) {
            assert_eq!(
                materials.get(&handle.0).unwrap().alpha_mode,
                AlphaMode::Opaque
            );
        }
    }

    #[test]
    fn different_document_layouts_retain_meshes_and_still_retire_changed_incarnations() {
        let mut app = interface_scene_fixture();
        let mut first = instance_cache_model("first", &[41, 42]);
        let second = instance_cache_model("second", &[73]);
        apply_interface_model(app.world_mut(), first.clone()).unwrap();
        let first_rows =
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["first"].clone();
        let first_cache = app
            .world()
            .resource::<ModelResource>()
            .cache_entity
            .unwrap();
        let first_edges_changed = app
            .world()
            .entity(first_cache)
            .get_ref::<ModelEdgeCache>()
            .unwrap()
            .last_changed();
        apply_interface_model(app.world_mut(), second.clone()).unwrap();
        let second_rows =
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["second"].clone();
        assert_eq!(first_rows.as_array().unwrap().len(), 2);
        assert_eq!(second_rows.as_array().unwrap().len(), 1);
        for _ in 0..3 {
            app.world_mut().increment_change_tick();
            for model in [&first, &second] {
                apply_interface_model(app.world_mut(), (*model).clone()).unwrap();
                let snapshot = interface_geometry_fixture_snapshot(app.world_mut());
                assert_eq!(snapshot["sessions"]["first"], first_rows);
                assert_eq!(snapshot["sessions"]["second"], second_rows);
            }
        }

        assert_eq!(
            app.world()
                .entity(first_cache)
                .get_ref::<ModelEdgeCache>()
                .unwrap()
                .last_changed(),
            first_edges_changed,
            "Tab switches retain local edge metadata"
        );
        apply_interface_model(app.world_mut(), first.clone()).unwrap();
        Arc::make_mut(&mut first.instance_body_poses)[0].translation[0] = 20.;
        apply_interface_view(
            app.world_mut(),
            "first",
            None,
            Some(ViewportPresentation {
                instance_body_poses: first.instance_body_poses.clone(),
                ..default()
            }),
        )
        .unwrap();
        assert_eq!(
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["first"],
            first_rows
        );

        Arc::make_mut(&mut first.instance_body_poses)[1].visible = false;
        apply_interface_view(
            app.world_mut(),
            "first",
            None,
            Some(ViewportPresentation {
                instance_body_poses: first.instance_body_poses.clone(),
                ..default()
            }),
        )
        .unwrap();
        let hidden_rows =
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["first"].clone();
        assert_eq!(hidden_rows.as_array().unwrap().len(), 1);
        assert_ne!(hidden_rows, first_rows);
        assert_eq!(
            hidden_rows[0]["mesh"], first_rows[0]["mesh"],
            "Occurrence visibility changes reuse the body's mesh upload"
        );
        assert_eq!(
            app.world()
                .entity(first_cache)
                .get_ref::<ModelEdgeCache>()
                .unwrap()
                .last_changed(),
            first_edges_changed
        );
        apply_interface_model(app.world_mut(), second.clone()).unwrap();
        interface_geometry_fixture_snapshot(app.world_mut());
        apply_interface_model(app.world_mut(), first.clone()).unwrap();
        assert_eq!(
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["first"],
            hidden_rows
        );

        first.geometry_revision += 1;
        Arc::make_mut(&mut Arc::make_mut(&mut first.document).scene).bodies[0]
            .mesh
            .positions[0] = 3.;
        apply_interface_model(app.world_mut(), first.clone()).unwrap();
        let edited_rows =
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["first"].clone();
        assert_ne!(edited_rows, hidden_rows);
        assert_ne!(
            edited_rows[0]["mesh"], hidden_rows[0]["mesh"],
            "A geometry edit replaces the uploaded mesh"
        );
        let mut isolated = first.clone();
        Arc::make_mut(&mut Arc::make_mut(&mut isolated.document).scene).bodies[0]
            .mesh
            .positions[0] = 9.;
        apply_interface_edit_model(app.world_mut(), isolated).unwrap();
        let transient_rows =
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["first"].clone();
        assert_ne!(transient_rows, edited_rows);
        apply_interface_model(app.world_mut(), second).unwrap();
        assert_eq!(
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["second"],
            second_rows
        );
        apply_interface_model(app.world_mut(), first.clone()).unwrap();
        assert_ne!(
            interface_geometry_fixture_snapshot(app.world_mut())["sessions"]["first"],
            transient_rows
        );
        assert!(!app.world().resource::<ModelResource>().transient_model);

        let retired_revision = app.world().resource::<ModelResource>().instance_revision;
        retire_interface_model_session(app.world_mut(), "first");
        let remaining = interface_geometry_fixture_snapshot(app.world_mut());
        assert!(remaining["sessions"].get("first").is_none());
        assert!(remaining["cache"].get("first").is_none());
        assert!(
            app.world().get_entity(first_cache).is_err(),
            "Retirement releases document-owned CPU metadata and mesh handles"
        );
        let states = &app.world().resource::<ModelResource>().instance_states;
        assert!(!states.contains_key("first"));
        assert_eq!(states.len(), 1);
        assert_eq!(remaining["sessions"]["second"], second_rows);
        apply_interface_model(app.world_mut(), first).unwrap();
        assert_ne!(
            app.world().resource::<ModelResource>().instance_revision,
            retired_revision
        );
    }
}
