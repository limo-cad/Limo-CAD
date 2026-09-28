//! Bevy CAD rendering and native desktop controls over the shared engine.

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod path_progress;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod platform;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(crate) use platform::script_preview;
pub(crate) use platform::physical_pick;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub mod interface_shell;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod profile_outline;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(crate) mod screenshot;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub mod ui;
mod preview_color;
pub(crate) use preview_color::ViewportColorRole;
pub(crate) mod localization;
pub(crate) mod system_locale;
#[cfg(test)]
pub(crate) use platform::{interface_scene_fixture, interface_geometry_fixture_snapshot};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(crate) use platform::{
    interface_camera_snapshot,
    interface_geometry,
    interface_model_revision,
    apply_interface_model, apply_interface_edit_model, apply_interface_preview, apply_interface_view, interface_body_transform,
    interface_pick, interface_preview_revision, interface_preview_snapshot, interface_sketch_point,
    interface_view_snapshot, interface_visible_occurrences, interface_world_point,
};
pub(crate) use platform::{apply_interface_cam_stock, interface_cam_stock_snapshot, apply_interface_sketch_lines, apply_interface_viewport, interface_support_pick, apply_interface_palette, interface_navigation_source, retire_interface_model_session};
#[cfg(all(
    any(target_os = "macos", target_os = "windows", target_os = "linux"),
    feature = "dev-ui-lab"
))]
pub mod ui_lab;
pub mod winit_host;

use nbcad_core::BodyAppearance;
use nbcad_sketch::{BodyPoseDto, InstanceBodyPoseDto, SketchDto};
use nbcad_solid::{
    DatumPlaneDefinitionDto, Point3Dto, ProfileCatalogItemDto, ProfileRefDto, SketchPointRefDto,
    SolidSceneDto,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub corner_radius: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportLayout {
    #[serde(default)]
    pub revision: u64,
    pub viewport: ViewportRect,
    #[serde(default)]
    pub overlays: Vec<ViewportRect>,
    #[serde(default)]
    pub palette: ViewportPalette,
    #[serde(default)]
    pub hud: ViewportHud,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportPalette {
    pub background: [f32; 3],
    pub panel: [f32; 3],
    pub header: [f32; 3],
    pub ui_edge: [f32; 3],
    pub ink: [f32; 3],
    pub mute: [f32; 3],
    pub accent: [f32; 3],
    pub grid_fine: [f32; 3],
    pub grid_major: [f32; 3],
    pub body: [f32; 3],
    pub body_selected: [f32; 3],
    pub body_tool: [f32; 3],
    pub body_selected_edge: [f32; 3],
    pub face_hover: [f32; 3],
    pub face_selected: [f32; 3],
    pub edge: [f32; 3],
    pub edge_hover: [f32; 3],
    pub edge_selected: [f32; 3],
    pub pick_halo: [f32; 3],
    pub origin_plane_xy: [f32; 3],
    pub origin_plane_xz: [f32; 3],
    pub origin_plane_yz: [f32; 3],
    pub active_sketch: [f32; 3],
    pub defined_sketch: [f32; 3],
    pub hover: [f32; 3],
    pub selection: [f32; 3],
    /// Sketch entities referenced by the UI-selected geometric constraint.
    pub constraint_related: [f32; 3],
    pub finished_sketch: [f32; 3],
    pub finished_sketch_point: [f32; 3],
    pub finished_sketch_point_outline: [f32; 3],
    pub preview: [f32; 3],
    #[serde(default = "default_dimension_color")]
    pub dimension: [f32; 3],
    /// Support-face boundary projected into the active sketch. Read-only
    /// reference geometry, never a pick target.
    pub projected: [f32; 3],
}

impl Default for ViewportPalette {
    fn default() -> Self {
        Self {
            background: [42.0 / 255.0, 45.0 / 255.0, 51.0 / 255.0],
            panel: [35.0 / 255.0, 38.0 / 255.0, 43.0 / 255.0],
            header: [43.0 / 255.0, 46.0 / 255.0, 53.0 / 255.0],
            ui_edge: [58.0 / 255.0, 62.0 / 255.0, 70.0 / 255.0],
            ink: [215.0 / 255.0, 220.0 / 255.0, 226.0 / 255.0],
            mute: [154.0 / 255.0, 160.0 / 255.0, 168.0 / 255.0],
            accent: [116.0 / 255.0, 99.0 / 255.0, 216.0 / 255.0],
            grid_fine: [58.0 / 255.0, 63.0 / 255.0, 71.0 / 255.0],
            grid_major: [77.0 / 255.0, 84.0 / 255.0, 95.0 / 255.0],
            body: [170.0 / 255.0, 190.0 / 255.0, 209.0 / 255.0],
            body_selected: [169.0 / 255.0, 103.0 / 255.0, 37.0 / 255.0],
            body_tool: [181.0 / 255.0, 138.0 / 255.0, 67.0 / 255.0],
            body_selected_edge: [1.0, 208.0 / 255.0, 0.0],
            face_hover: [35.0 / 255.0, 138.0 / 255.0, 157.0 / 255.0],
            face_selected: [207.0 / 255.0, 119.0 / 255.0, 21.0 / 255.0],
            edge: [41.0 / 255.0, 51.0 / 255.0, 61.0 / 255.0],
            edge_hover: [0.0, 245.0 / 255.0, 1.0],
            edge_selected: [1.0, 208.0 / 255.0, 0.0],
            pick_halo: [1.0, 1.0, 1.0],
            origin_plane_xy: [87.0 / 255.0, 168.0 / 255.0, 1.0],
            origin_plane_xz: [85.0 / 255.0, 201.0 / 255.0, 120.0 / 255.0],
            origin_plane_yz: [1.0, 112.0 / 255.0, 120.0 / 255.0],
            active_sketch: [134.0 / 255.0, 169.0 / 255.0, 199.0 / 255.0],
            defined_sketch: [232.0 / 255.0, 233.0 / 255.0, 236.0 / 255.0],
            hover: [0.0, 245.0 / 255.0, 1.0],
            selection: [1.0, 208.0 / 255.0, 0.0],
            constraint_related: [62.0 / 255.0, 207.0 / 255.0, 154.0 / 255.0],
            finished_sketch: [134.0 / 255.0, 169.0 / 255.0, 199.0 / 255.0],
            finished_sketch_point: [134.0 / 255.0, 169.0 / 255.0, 199.0 / 255.0],
            finished_sketch_point_outline: [21.0 / 255.0, 25.0 / 255.0, 31.0 / 255.0],
            preview: [143.0 / 255.0, 196.0 / 255.0, 1.0],
            dimension: default_dimension_color(),
            projected: [192.0 / 255.0, 140.0 / 255.0, 245.0 / 255.0],
        }
    }
}

fn default_dimension_color() -> [f32; 3] { [174.0 / 255.0, 203.0 / 255.0, 30.0 / 255.0] }

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportMode {
    #[default]
    Solid,
    PickPlane,
    Sketch,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportOriginPlane {
    Xy,
    Xz,
    Yz,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportPresentation {
    /// Presentation-only palette choices; hiding a point never deletes it or
    /// disables snapping. False preserves existing clients' display defaults.
    #[serde(default)]
    pub hide_sketch_grid: bool,
    #[serde(default)]
    pub hide_sketch_points: bool,
    #[serde(default)]
    pub mode: ViewportMode,
    pub hovered_origin_plane: Option<ViewportOriginPlane>,
    pub hovered_datum_plane_id: Option<u64>,
    pub selected_origin_plane: Option<ViewportOriginPlane>,
    pub selected_datum_plane_id: Option<u64>,
    #[serde(default)]
    pub selected_body_ids: Vec<u64>,
    pub selected_occurrence_id: Option<u64>,
    pub hovered_occurrence_id: Option<u64>,
    pub hovered_body_id: Option<u64>,
    #[serde(default)]
    pub selected_face_ids: Vec<u64>,
    pub hovered_face_id: Option<u64>,
    #[serde(default)]
    pub selected_edge_ids: Vec<u64>,
    pub hovered_edge_id: Option<u64>,
    #[serde(default)]
    pub pick_refinable_edges: bool,
    /// Sketch-palette "Projected Geometries" visibility toggle. Inverted so
    /// the derived `Default` (an older payload that predates the toggle) keeps
    /// the reference geometry visible. The projected support-face boundary is
    /// never a pick target, so hiding it cannot change selection state.
    #[serde(default)]
    pub hide_projected_geometry: bool,
    #[serde(default)]
    pub pick_straight_edges: bool,
    #[serde(default)]
    pub selected_sketch_entity_ids: Vec<u64>,
    /// Entities referenced by the UI-selected geometric constraint. Distinct
    /// from selection so Bevy can paint a related-highlight color.
    #[serde(default)]
    pub constraint_related_sketch_entity_ids: Vec<u64>,
    pub hovered_sketch_entity_id: Option<u64>,
    #[serde(default)]
    pub selected_finished_sketch_entities: Vec<FinishedSketchEntityPickRef>,
    pub hovered_finished_sketch_entity: Option<FinishedSketchEntityPickRef>,
    #[serde(default)]
    pub selected_sketch_points: Vec<SketchPointRefDto>,
    pub hovered_sketch_point: Option<SketchPointRefDto>,
    pub selected_surface_point: Option<Point3Dto>,
    pub hovered_surface_point: Option<Point3Dto>,
    #[serde(default)]
    pub hidden_body_ids: Vec<u64>,
    /// Bodies rendered as a faint shell with see-through wireframe edges:
    /// CAM uses this only when the operator explicitly adds the finished
    /// target as an X-Ray reference over the remaining-stock stage.
    #[serde(default)]
    pub ghosted_body_ids: Vec<u64>,
    #[serde(default)]
    pub hidden_datum_plane_ids: Vec<u64>,
    #[serde(default)]
    pub hidden_sketch_names: Vec<String>,
    #[serde(default)]
    pub profile_picker_active: bool,
    #[serde(default)]
    pub selected_profiles: Vec<ProfileRefDto>,
    #[serde(default)]
    pub candidate_profiles: Vec<ProfileRefDto>,
    pub hovered_profile: Option<ProfileRefDto>,
    /// Host-neutral rigid poses. Kept with the small presentation stream so
    /// live motion never clones or retessellates the OCCT scene.
    #[serde(default)]
    pub body_poses: Vec<BodyPoseDto>,
    /// Per-occurrence display rows; several rows may reuse one source body.
    #[serde(default)]
    pub instance_body_poses: Vec<InstanceBodyPoseDto>,
    /// Desktop CAM simulation stock is retained directly by Bevy rather than
    /// travelling through the webview's transient preview JSON.
    #[serde(default)]
    pub cam_stock_visible: bool,
    /// Retained CAM cutter primitive. Playback updates only its pose and
    /// dimensions; stock/path triangle soup stays in the static preview.
    pub cam_tool: Option<ViewportCamTool>,
    /// Lightweight playback cursor for retained, time-tagged path segments.
    pub cam_path_progress: Option<ViewportCamPathProgress>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportCamPathProgress {
    pub path_id: u64,
    pub time_seconds: f64,
    /// Physical cutter tip/centerline in model coordinates, including arcs.
    pub position: [f32; 3],
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportCamTool {
    pub tip: [f32; 3],
    pub axis: [f32; 3],
    pub geometry: nbcad_cam::CamCutterGeometryDto,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FinishedSketchEntityPickRef {
    pub sketch_name: String,
    pub entity_id: u64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportHudRow {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportHudSelection {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub rows: Vec<ViewportHudRow>,
    pub footer: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportHud {
    #[serde(default)]
    pub render_native_chrome: bool,
    #[serde(default = "default_nav_tool")]
    pub nav_tool: String,
    #[serde(default)]
    pub sketch_mode: bool,
    #[serde(default)]
    pub can_undo: bool,
    #[serde(default)]
    pub can_redo: bool,
    #[serde(default)]
    pub six_dof_state: String,
    #[serde(default)]
    pub hovered_control: String,
    #[serde(default)]
    pub pressed_control: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub dof_label: Option<String>,
    #[serde(default)]
    pub coordinate_readout: Option<String>,
    #[serde(default)]
    pub dim_opacity: f32,
    pub selection: Option<ViewportHudSelection>,
}

fn default_nav_tool() -> String {
    "select".to_string()
}

impl Default for ViewportHud {
    fn default() -> Self {
        Self {
            render_native_chrome: false,
            nav_tool: default_nav_tool(),
            sketch_mode: false,
            can_undo: false,
            can_redo: false,
            six_dof_state: "disconnected".to_string(),
            hovered_control: String::new(),
            pressed_control: String::new(),
            prompt: None,
            dof_label: None,
            coordinate_readout: None,
            dim_opacity: 0.0,
            selection: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportCamera {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub vertical_fov_degrees: f32,
}

/// 90 mm full-frame-equivalent vertical field of view.
const DEFAULT_VERTICAL_FOV_DEGREES: f32 = 15.2;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportLinePattern {
    #[default]
    Solid,
    Dotted,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportLineLayer {
    /// sRGBA from the DOM theme/presentation material.
    #[serde(default)]
    pub color: [f32; 4],
    /// Native preview roles follow palette changes without a geometry query.
    /// The React bridge's explicit RGBA contract is unchanged.
    #[serde(skip)]
    pub(crate) color_role: ViewportColorRole,
    /// Requested screen-space width. Bevy maps this to its normal/highlight
    /// gizmo pipelines rather than treating it as a world-space measurement.
    #[serde(default = "default_line_width")]
    pub width: f32,
    /// Semantic construction-guide styling. Older callers deserialize as a
    /// solid line, while native rendering keeps dotted spacing screen-sized.
    #[serde(default)]
    pub pattern: ViewportLinePattern,
    /// World-space line segments, packed as x0, y0, z0, x1, y1, z1.
    #[serde(default)]
    pub segments: Vec<f32>,
    /// CAM-only timing; absent on ordinary modeling/selection guides.
    pub playback: Option<ViewportLinePlayback>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportLinePlayback {
    pub path_id: u64,
    pub completed_color: [f32; 4],
    /// Start/end seconds per line segment, retained at timeline creation.
    pub segment_times: Vec<f64>,
}

impl ViewportLinePlayback {
    fn is_valid_for(&self, segment_floats: usize) -> bool {
        segment_floats.is_multiple_of(6)
            && self.segment_times.len() == segment_floats / 3
            && self.completed_color.iter().all(|value| value.is_finite())
            && self
                .segment_times
                .chunks_exact(2)
                .all(|pair| pair[0].is_finite() && pair[1].is_finite() && pair[1] >= pair[0])
    }
}

fn default_line_width() -> f32 {
    1.0
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportPointLayer {
    /// sRGBA from the DOM theme/presentation material.
    #[serde(default)]
    pub color: [f32; 4],
    #[serde(skip)]
    pub(crate) color_role: ViewportColorRole,
    /// Approximate world-space marker radius derived from the current camera.
    #[serde(default)]
    pub radius: f32,
    /// Hollow means this marker retains solver freedom; filled means fully
    /// constrained (or a command-specific solid handle).
    #[serde(default)]
    pub hollow: bool,
    /// World-space point positions, packed as x, y, z.
    #[serde(default)]
    pub positions: Vec<f32>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportTriangleLayer {
    /// sRGBA fill color. Positions are an already-triangulated world-space
    /// triangle list because profile topology is owned by the command layer.
    #[serde(default)]
    pub color: [f32; 4],
    #[serde(default)]
    pub positions: Vec<f32>,
    /// Optional world-space vertex normals, packed one-for-one with
    /// positions. When omitted the native renderer computes flat normals.
    #[serde(default)]
    pub normals: Vec<f32>,
    /// Physical CAM stock uses the normal lit/depth-writing model pipeline;
    /// command fills retain their translucent unlit overlay presentation.
    #[serde(default)]
    pub material: ViewportTriangleMaterial,
    /// Draw after model depth for internal datum/profile selection.
    #[serde(default)]
    pub xray: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportTriangleMaterial {
    #[default]
    Overlay,
    MachinedStock,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportArrow {
    #[serde(default)]
    pub start: [f32; 3],
    #[serde(default)]
    pub end: [f32; 3],
    #[serde(default)]
    pub color: [f32; 4],
    #[serde(default = "default_line_width")]
    pub width: f32,
    #[serde(default)]
    pub xray: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportAnnotationKind {
    #[default]
    Dimension,
    Constraint,
    Tool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportConstraintIcon {
    HorizontalVertical,
    Horizontal,
    Vertical,
    HorizontalPoints,
    VerticalPoints,
    Coincident,
    Tangent,
    Equal,
    Parallel,
    Perpendicular,
    Fix,
    Midpoint,
    Concentric,
    /// A point glued to an arc's implicit start/end. Every variant here must
    /// stay in step with `ConstraintIconKind` in
    /// `src/sketch/constraintIcons.tsx`: an unknown variant makes serde reject
    /// the whole transient preview, which freezes the cursor HUD on screen.
    ArcEndpoint,
    Collinear,
    Symmetry,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportToolIcon {
    Line,
    MidpointLine,
    Point,
    Rectangle,
    Circle,
    Arc,
    Dimension,
    Fillet,
    Chamfer,
    Offset,
    Trim,
    Extend,
    Break,
    Mirror,
    MoveCopy,
    Scale,
    Polygon,
    Slot,
    Spline,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportAnnotation {
    /// Viewport-local logical pixels. React already owns the exact projection
    /// used for picking, so annotations stay aligned with its interaction
    /// scene during orbit, resize, and DPI changes.
    #[serde(default)]
    pub screen: [f32; 2],
    #[serde(default)]
    pub color: [f32; 4],
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub kind: ViewportAnnotationKind,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub icon: Option<ViewportConstraintIcon>,
    #[serde(default)]
    pub tool_icon: Option<ViewportToolIcon>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewportSnapKind {
    #[default]
    Grid,
    Origin,
    Point,
    Midpoint,
    ReferenceMidpoint,
    Curve,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewportSnapMarker {
    pub position: [f32; 3],
    #[serde(default)]
    pub kind: ViewportSnapKind,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewportPreview {
    /// Small, transient presentation layers only. Committed sketches and OCCT
    /// tessellations continue to travel through the direct Rust snapshot path.
    #[serde(default)]
    pub lines: Vec<ViewportLineLayer>,
    #[serde(default)]
    pub points: Vec<ViewportPointLayer>,
    #[serde(default)]
    pub triangles: Vec<ViewportTriangleLayer>,
    #[serde(default)]
    pub arrows: Vec<ViewportArrow>,
    #[serde(default)]
    pub annotations: Vec<ViewportAnnotation>,
    /// Optional semantic, world-space sketch snap marker. Keeping the kind
    /// prevents the native viewport from flattening endpoints, midpoints,
    /// origins, and ordinary grid acquisition into one ambiguous crosshair.
    pub marker: Option<ViewportSnapMarker>,
}

impl Default for ViewportCamera {
    fn default() -> Self {
        Self {
            position: [170.0, -170.0, 130.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            vertical_fov_degrees: DEFAULT_VERTICAL_FOV_DEGREES,
        }
    }
}

/// Ordinary selection must hit physical geometry. Joint creation explicitly
/// opts into virtual cylinder openings and analytic connector targets.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NativePickPurpose {
    #[default]
    Geometry,
    JointConnector,
    RefinableEdge,
    Edge,
    StraightEdge,
    Vertex,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePick {
    pub body_id: u64,
    pub occurrence_id: Option<u64>,
    pub face_id: u64,
    pub edge_id: Option<u64>,
    pub point: [f32; 3],
    pub distance: f32,
    pub connector_kind: Option<String>,
    pub connector_origin: Option<[f32; 3]>,
    pub connector_primary_axis: Option<[f32; 3]>,
    pub connector_secondary_axis: Option<[f32; 3]>,
    pub connector_radius: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeViewportMetrics {
    pub available: bool,
    pub ready: bool,
    pub startup_error: Option<String>,
    pub backend: String,
    pub logical_width: f64,
    pub logical_height: f64,
    pub scale_factor: f64,
    pub physical_width: u32,
    pub physical_height: u32,
    pub rendered_frames: u64,
    pub wakeups: u64,
    pub average_frame_ms: f64,
    pub last_pointer_latency_ms: f64,
    pub body_count: usize,
    pub triangle_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ViewportModel {
    pub session_id: String,
    pub geometry_revision: u64,
    pub scene: SolidSceneDto,
    pub active_sketch: Option<SketchDto>,
    pub finished_sketches: Vec<SketchDto>,
    pub datum_planes: Vec<DatumPlaneDefinitionDto>,
    pub profile_catalog: Vec<ProfileCatalogItemDto>,
    pub body_appearances: Vec<BodyAppearance>,
    pub body_poses: Vec<BodyPoseDto>,
    pub instance_body_poses: Vec<InstanceBodyPoseDto>,
}

/// Borrowed rendered geometry for framing; this also includes isolated feature
/// edit inputs, and never copies the meshes to move a camera.
pub(crate) struct ViewportGeometry<'a> {
    pub scene: &'a SolidSceneDto,
    pub active_sketch: Option<&'a SketchDto>,
    pub finished_sketches: &'a [SketchDto],
    pub instance_body_poses: &'a [InstanceBodyPoseDto],
}
impl<'a> From<&'a ViewportModel> for ViewportGeometry<'a> {
    fn from(model: &'a ViewportModel) -> Self {
        Self { scene:&model.scene, active_sketch:model.active_sketch.as_ref(), finished_sketches:&model.finished_sketches, instance_body_poses:&model.instance_body_poses }
    }
}

/// Remaining-stock surface already transformed into model/world coordinates.
/// This is an internal Rust-to-Bevy channel: it deliberately has no serde
/// contract because the webview must never relay these large buffers.
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewportCamStock {
    pub positions: std::sync::Arc<Vec<f32>>,
    pub normals: std::sync::Arc<Vec<f32>>,
}

