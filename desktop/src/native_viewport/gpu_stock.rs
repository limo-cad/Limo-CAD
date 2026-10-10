//! GPU stock removal during CAM playback.
//!
//! The CPU voxel simulation stays the authority for removal, volumes and
//! verification; its retained surface arrives a few frames apart. Between
//! those frames the GPU keeps a height field of the lowest cutter surface
//! that has passed over each column since the retained frame and the
//! renderer shows:
//!
//! - the retained stock with everything above that surface discarded, and
//! - the new floors and walls (the surface itself) inside the retained
//!   stock's column, from a CPU-rasterized top/bottom map of that frame.
//!
//! A new retained frame or a rewind clears the field and restamps the path
//! traveled since that frame; otherwise each frame stamps only new travel.
//! Removal falls back to the retained CPU stock unless every stamped cutter's
//! finite flute reaches above the whole stock in the tool-axis frame.
//! Tool-change and ambiguous timelines retain CPU display until exact timed
//! cutter profiles are available; GPU removal requires one known tool.

mod flute;
mod timeline;
mod visibility;

use bevy::{
    asset as bevy_asset,
    asset::{uuid_handle, AssetApp, RenderAssetUsages},
    camera::visibility::NoFrustumCulling,
    core_pipeline::schedule::camera_driver,
    extract as bevy_extract, image as bevy_image,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::Indices,
    pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin},
    prelude::*,
    render as bevy_render,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{storage_buffer_read_only, texture_storage_2d, uniform_buffer},
            encase, AsBindGroup, BindGroupEntries, BindGroupLayoutDescriptor,
            BindGroupLayoutEntries, CachedComputePipelineId, ComputePassDescriptor,
            ComputePipelineDescriptor, Extent3d, PipelineCache, PrimitiveTopology, ShaderStages,
            ShaderType, StorageBuffer, StorageTextureAccess, TextureDimension, TextureFormat,
            TextureUsages, UniformBuffer,
        },
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue},
        texture::GpuImage,
        Render, RenderApp, RenderSystems,
    },
    shader::{Shader, ShaderRef},
};
use limo_cad_cam::{CamCutterGeometryDto, CutterProfile};
use std::borrow::Cow;

use super::{ViewportCamPathProgress, ViewportCamTool, ViewportLineLayer};

const COMMON_SHADER: Handle<Shader> = uuid_handle!("6b1f4d3e-8c52-4f0a-9d7e-2a6f1c9b5e01");
const STAMP_SHADER: Handle<Shader> = uuid_handle!("6b1f4d3e-8c52-4f0a-9d7e-2a6f1c9b5e02");
const CLIP_SHADER: Handle<Shader> = uuid_handle!("6b1f4d3e-8c52-4f0a-9d7e-2a6f1c9b5e03");
const CUT_SHADER: Handle<Shader> = uuid_handle!("6b1f4d3e-8c52-4f0a-9d7e-2a6f1c9b5e04");

/// Texels along the longer side of the stock. Also the cut-surface grid.
const MAX_FIELD_TEXELS: u32 = 768;
const PROFILE_SAMPLES: usize = 64;
const UNCUT: f32 = 1.0e9;
const WORKGROUP: u32 = 8;

pub(super) type StockClipMaterial = ExtendedMaterial<StandardMaterial, StockClip>;
pub(super) type CutSurfaceMaterial = ExtendedMaterial<StandardMaterial, CutSurface>;

/// See `common.wesl` for the meaning of each lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType, Reflect)]
pub(super) struct FieldFrame {
    origin: Vec4,
    x_axis: Vec4,
    y_axis: Vec4,
    z_axis: Vec4,
    grid: Vec4,
}

/// Retained CPU stock surface with GPU removal applied.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct StockClip {
    #[uniform(100)]
    frame: FieldFrame,
    #[texture(101, sample_type = "float", filterable = false)]
    field: Handle<Image>,
    #[texture(102, sample_type = "float", filterable = false)]
    columns: Handle<Image>,
}

impl MaterialExtension for StockClip {
    fn fragment_shader() -> ShaderRef {
        CLIP_SHADER.into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// Floors and walls cut since the retained frame.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct CutSurface {
    #[uniform(100)]
    frame: FieldFrame,
    #[texture(
        101,
        sample_type = "float",
        filterable = false,
        visibility(vertex, fragment)
    )]
    field: Handle<Image>,
    #[texture(
        102,
        sample_type = "float",
        filterable = false,
        visibility(vertex, fragment)
    )]
    columns: Handle<Image>,
}

impl MaterialExtension for CutSurface {
    fn vertex_shader() -> ShaderRef {
        CUT_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        CUT_SHADER.into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

pub(super) struct GpuStockPlugin;

impl Plugin for GpuStockPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<Assets<Shader>>() {
            app.init_asset::<Shader>();
        }
        if !app.world().contains_resource::<Assets<Image>>() {
            app.init_asset::<Image>();
        }
        let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
        for (handle, source, path) in [
            (
                COMMON_SHADER,
                include_str!("gpu_stock/common.wesl"),
                "embedded://limo_cad/gpu_stock/common.wesl",
            ),
            (
                STAMP_SHADER,
                include_str!("gpu_stock/stamp.wesl"),
                "embedded://limo_cad/gpu_stock/stamp.wesl",
            ),
            (
                CLIP_SHADER,
                include_str!("gpu_stock/clip.wesl"),
                "embedded://limo_cad/gpu_stock/clip.wesl",
            ),
            (
                CUT_SHADER,
                include_str!("gpu_stock/cut.wesl"),
                "embedded://limo_cad/gpu_stock/cut.wesl",
            ),
        ] {
            let _ = shaders.insert(handle.id(), Shader::from_wesl(source, path));
        }
        app.add_plugins((
            MaterialPlugin::<StockClipMaterial>::default(),
            MaterialPlugin::<CutSurfaceMaterial>::default(),
        ))
        .init_resource::<GpuStockStamp>();
        if app.get_sub_app(RenderApp).is_some() {
            app.add_plugins(ExtractResourcePlugin::<GpuStockStamp>::default());
        }
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<StampProgress>()
                .add_systems(
                    Render,
                    init_stamp_pipeline
                        .in_set(RenderSystems::PrepareResources)
                        .before(RenderSystems::Render),
                )
                .add_systems(RenderGraph, stamp_field.before(camera_driver));
        }
    }
}

/// Field coordinates: an orthonormal basis whose z is the tool axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frame {
    origin: Vec3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
    /// Field-frame XY of texel (0, 0)'s corner.
    min: Vec2,
    texel: f32,
    dims: UVec2,
    /// Field-frame height drawn for uncut texels: above the whole stock.
    top: f32,
}

impl Frame {
    /// Fit the field to the retained stock's extent across the tool axis.
    pub(super) fn fit(positions: &[f32], axis: Vec3) -> Option<Self> {
        let z = axis.try_normalize()?;
        let x = z.any_orthonormal_vector();
        let y = z.cross(x);
        let origin = Vec3::ZERO;
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for point in positions.as_chunks::<3>().0.iter() {
            let p = Vec3::new(point[0], point[1], point[2]);
            if !p.is_finite() {
                return None;
            }
            let local = Vec3::new(p.dot(x), p.dot(y), p.dot(z));
            low = low.min(local);
            high = high.max(local);
        }
        if !low.is_finite() || !high.is_finite() {
            return None;
        }
        let extent = (high - low).truncate();
        let longest = extent.max_element();
        if longest <= 0.0 {
            return None;
        }
        let texel = longest / MAX_FIELD_TEXELS as f32;
        if !texel.is_finite() || texel <= 0.0 {
            return None;
        }
        let min = low.truncate() - Vec2::splat(texel);
        let dims = ((extent / texel).ceil().as_uvec2() + UVec2::splat(2))
            .clamp(UVec2::ONE, UVec2::splat(MAX_FIELD_TEXELS + 2));
        let top = high.z + texel.max(1.0);
        if !top.is_finite() || top <= high.z || top >= UNCUT {
            return None;
        }
        Some(Self {
            origin,
            x,
            y,
            z,
            min,
            texel,
            dims,
            top,
        })
    }

    fn local(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    fn uniform(&self, active: bool) -> FieldFrame {
        FieldFrame {
            origin: self.origin.extend(if active { 1.0 } else { 0.0 }),
            x_axis: self.x.extend(self.texel),
            y_axis: self.y.extend(self.texel * 0.25),
            z_axis: self.z.extend(self.top),
            grid: Vec4::new(
                self.min.x,
                self.min.y,
                self.dims.x as f32,
                self.dims.y as f32,
            ),
        }
    }
}

/// Top and bottom of the retained stock surface over each texel center.
/// Columns the surface does not cover hold an empty interval.
pub(super) fn rasterize_columns(positions: &[f32], frame: &Frame) -> Vec<[f32; 2]> {
    let (width, height) = (frame.dims.x as usize, frame.dims.y as usize);
    let mut columns = vec![[f32::NEG_INFINITY, f32::INFINITY]; width * height];
    for triangle in positions.as_chunks::<9>().0.iter() {
        let p: [Vec3; 3] = std::array::from_fn(|i| {
            frame.local(Vec3::new(
                triangle[i * 3],
                triangle[i * 3 + 1],
                triangle[i * 3 + 2],
            ))
        });
        let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y);
        if area.abs() <= f32::EPSILON * 16.0 {
            continue;
        }
        let low = p[0].truncate().min(p[1].truncate()).min(p[2].truncate());
        let high = p[0].truncate().max(p[1].truncate()).max(p[2].truncate());
        let to_texel = |v: f32, min: f32| (v - min) / frame.texel - 0.5;
        let x0 = to_texel(low.x, frame.min.x).ceil().max(0.0) as usize;
        let y0 = to_texel(low.y, frame.min.y).ceil().max(0.0) as usize;
        let x1 = (to_texel(high.x, frame.min.x).floor() as isize).min(width as isize - 1);
        let y1 = (to_texel(high.y, frame.min.y).floor() as isize).min(height as isize - 1);
        if x1 < 0 || y1 < 0 {
            continue;
        }
        for ty in y0..=y1 as usize {
            for tx in x0..=x1 as usize {
                let q = frame.min + (Vec2::new(tx as f32, ty as f32) + 0.5) * frame.texel;
                let edge = |a: Vec3, b: Vec3| (b.x - a.x) * (q.y - a.y) - (q.x - a.x) * (b.y - a.y);
                let w0 = edge(p[1], p[2]) / area;
                let w1 = edge(p[2], p[0]) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                    continue;
                }
                let z = w0 * p[0].z + w1 * p[1].z + w2 * p[2].z;
                let column = &mut columns[tx + ty * width];
                column[0] = column[0].max(z);
                column[1] = column[1].min(z);
            }
        }
    }
    columns
}

/// Tip path segment in field coordinates with its cutter profile index.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub(super) struct GpuSegment {
    a: Vec4,
    b: Vec4,
}

#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub(super) struct GpuProfile {
    radius: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    heights: [Vec4; PROFILE_SAMPLES / 4],
}

/// Lowest cutter surface above the tip at evenly spaced radial distances,
/// from the analytic profile (flat land, corner radius, cone or bevel).
pub(super) fn profile_table(geometry: CamCutterGeometryDto) -> Option<GpuProfile> {
    let profile = CutterProfile::new(geometry).ok()?;
    let radius = geometry.diameter * 0.5;
    if !(radius as f32).is_finite() || radius as f32 <= 0.0 {
        return None;
    }
    let reach = geometry.flute_length;
    let mut samples = [0.0f32; PROFILE_SAMPLES];
    for (i, sample) in samples.iter_mut().enumerate() {
        let radial = radius * i as f64 / (PROFILE_SAMPLES - 1) as f64;
        let reaches = |z: f64| {
            profile
                .radius_at_height(z)
                .is_some_and(|r| r >= radial - 1e-9)
        };
        *sample = if reaches(0.0) {
            0.0
        } else if !reaches(reach) {
            UNCUT
        } else {
            let (mut low, mut high) = (0.0, reach);
            for _ in 0..40 {
                let mid = (low + high) * 0.5;
                if reaches(mid) {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            high as f32
        };
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return None;
    }
    Some(GpuProfile {
        radius: radius as f32,
        _pad0: 0.0,
        _pad1: 0.0,
        _pad2: 0.0,
        heights: std::array::from_fn(|i| Vec4::from_slice(&samples[i * 4..i * 4 + 4])),
    })
}

/// Tip travel of `path_id` between `since` and `cursor`, clipped at both
/// ends, in time order. `tool` names the profile index for a start time.
pub(super) fn traveled_segments(
    lines: &[ViewportLineLayer],
    path_id: u64,
    since: f64,
    cursor: f64,
    frame: &Frame,
    mut tool: impl FnMut(f64) -> Option<u32>,
) -> Vec<(f64, GpuSegment)> {
    let mut traveled = Vec::new();
    if cursor <= since {
        return traveled;
    }
    for layer in lines {
        let Some(playback) = layer
            .playback
            .as_ref()
            .filter(|playback| playback.path_id == path_id && playback.removes_stock)
        else {
            continue;
        };
        for (segment, times) in layer
            .segments
            .as_chunks::<6>()
            .0
            .iter()
            .zip(playback.segment_times.as_chunks::<2>().0.iter())
        {
            let (begin, end) = (times[0], times[1]);
            if end <= since || begin >= cursor {
                continue;
            }
            let a = Vec3::new(segment[0], segment[1], segment[2]);
            let b = Vec3::new(segment[3], segment[4], segment[5]);
            let at = |time: f64| {
                if end - begin <= f64::EPSILON {
                    b
                } else {
                    a.lerp(b, ((time - begin) / (end - begin)).clamp(0.0, 1.0) as f32)
                }
            };
            let start = begin.max(since);
            let Some(profile) = tool(start) else {
                continue;
            };
            traveled.push((
                start,
                GpuSegment {
                    a: frame.local(at(start)).extend(profile as f32),
                    b: frame.local(at(end.min(cursor))).extend(0.0),
                },
            ));
        }
    }
    traveled.sort_by(|a, b| a.0.total_cmp(&b.0));
    traveled
}

/// Main-world state of the GPU stock field.
#[derive(Resource)]
pub(super) struct GpuStock {
    pub clip: Handle<StockClipMaterial>,
    cut: Handle<CutSurfaceMaterial>,
    field: Handle<Image>,
    columns: Handle<Image>,
    grid: Option<(Entity, UVec2)>,
    frame: Option<Frame>,
    base_time: f64,
    stock_revision: Option<u64>,
    path_id: Option<u64>,
    /// Cutter observed at each playback time, for travel stamped later.
    tools: Vec<(f64, CamCutterGeometryDto)>,
    cursor: f64,
    reset_id: u64,
    active: bool,
}

/// Inputs from the native viewport's retained presentation.
pub(super) struct GpuStockInputs<'a> {
    pub enabled: bool,
    pub stock_positions: Option<&'a [f32]>,
    pub stock_time: Option<f64>,
    pub stock_revision: u64,
    pub cursor: Option<ViewportCamPathProgress>,
    pub tool: Option<ViewportCamTool>,
    pub lines: &'a [ViewportLineLayer],
}

/// Extracted to the render world whenever the stamped travel changes.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(RenderApp)]
pub(super) struct GpuStockStamp {
    field: Option<Handle<Image>>,
    dims: UVec2,
    min: Vec2,
    texel: f32,
    reset_id: u64,
    revision: u64,
    segments: Vec<GpuSegment>,
    profiles: Vec<GpuProfile>,
}

impl GpuStockStamp {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
}

fn placeholder(images: &mut Assets<Image>, format: TextureFormat, bytes: Vec<u8>) -> Handle<Image> {
    let mut image = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::STORAGE_BINDING;
    images.add(image)
}

impl GpuStock {
    pub(super) fn new(
        images: &mut Assets<Image>,
        clip_materials: &mut Assets<StockClipMaterial>,
        cut_materials: &mut Assets<CutSurfaceMaterial>,
        base: StandardMaterial,
    ) -> Self {
        let field = placeholder(
            images,
            TextureFormat::R32Float,
            UNCUT.to_le_bytes().to_vec(),
        );
        let columns = placeholder(
            images,
            TextureFormat::Rg32Float,
            [f32::NEG_INFINITY, f32::INFINITY]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
        );
        let clip = clip_materials.add(StockClipMaterial {
            base: base.clone(),
            extension: StockClip {
                frame: FieldFrame::default(),
                field: field.clone(),
                columns: columns.clone(),
            },
        });
        let cut = cut_materials.add(CutSurfaceMaterial {
            base,
            extension: CutSurface {
                frame: FieldFrame::default(),
                field: field.clone(),
                columns: columns.clone(),
            },
        });
        Self {
            clip,
            cut,
            field,
            columns,
            grid: None,
            frame: None,
            base_time: 0.0,
            stock_revision: None,
            path_id: None,
            tools: Vec::new(),
            cursor: f64::NEG_INFINITY,
            reset_id: 0,
            active: false,
        }
    }

    fn tool_index(&self, time: f64, tools: &mut Vec<CamCutterGeometryDto>) -> Option<u32> {
        let geometry = self
            .tools
            .iter()
            .rev()
            .find(|(observed, _)| *observed <= time)
            .or_else(|| self.tools.first())?
            .1;
        let index = match tools.iter().position(|tool| *tool == geometry) {
            Some(index) => index,
            None => {
                tools.push(geometry);
                tools.len() - 1
            }
        };
        Some(index as u32)
    }

    fn observe_tool(&mut self, time: f64, geometry: CamCutterGeometryDto) {
        let index = self
            .tools
            .partition_point(|(observed, _)| *observed <= time);
        if index > 0 && self.tools[index - 1].1 == geometry {
            return;
        }
        if self
            .tools
            .get(index)
            .is_some_and(|(_, tool)| *tool == geometry)
        {
            self.tools[index].0 = time;
            return;
        }
        self.tools.insert(index, (time, geometry));
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
        &mut self,
        inputs: GpuStockInputs,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        meshes: &mut Assets<Mesh>,
        clip_materials: &mut Assets<StockClipMaterial>,
        cut_materials: &mut Assets<CutSurfaceMaterial>,
        stamp: &mut GpuStockStamp,
        visibility: &mut Query<&mut Visibility>,
    ) {
        let ready = (|| {
            let cursor = inputs.cursor?;
            let tool = inputs.tool?;
            let time = inputs.stock_time?;
            let positions = inputs.stock_positions?;
            let single_tool = timeline::single_tool(
                inputs
                    .lines
                    .iter()
                    .filter_map(|layer| layer.playback.as_ref())
                    .filter(|playback| playback.path_id == cursor.path_id)
                    .map(|playback| playback.single_tool),
            );
            (inputs.enabled && single_tool).then_some((cursor, tool, time, positions))
        })();
        let Some((cursor, tool, base_time, positions)) = ready else {
            self.set_active(false, commands, clip_materials, cut_materials, visibility);
            if inputs.stock_positions.is_none() && self.frame.take().is_some() {
                if let Some((entity, _)) = self.grid.take() {
                    commands.entity(entity).despawn();
                }
                self.field = placeholder(
                    images,
                    TextureFormat::R32Float,
                    UNCUT.to_le_bytes().to_vec(),
                );
                self.columns = placeholder(
                    images,
                    TextureFormat::Rg32Float,
                    [f32::NEG_INFINITY, f32::INFINITY]
                        .into_iter()
                        .flat_map(f32::to_le_bytes)
                        .collect(),
                );
                if let Some(mut material) = clip_materials.get_mut(&self.clip) {
                    material.extension.field = self.field.clone();
                    material.extension.columns = self.columns.clone();
                }
                if let Some(mut material) = cut_materials.get_mut(&self.cut) {
                    material.extension.field = self.field.clone();
                    material.extension.columns = self.columns.clone();
                }
                self.tools.clear();
                self.path_id = None;
                *stamp = GpuStockStamp {
                    revision: stamp.revision.wrapping_add(1),
                    ..default()
                };
            }
            return;
        };
        if self.path_id != Some(cursor.path_id) {
            self.path_id = Some(cursor.path_id);
            self.tools.clear();
            self.stock_revision = None;
        }
        self.observe_tool(cursor.time_seconds, tool.geometry);
        let axis = Vec3::from_array(tool.axis);
        let frame_changed = self
            .frame
            .is_none_or(|frame| (frame.z - axis.normalize_or_zero()).length_squared() > 1e-10);
        let rebased = self.stock_revision != Some(inputs.stock_revision) || frame_changed;
        if rebased {
            let Some(frame) = Frame::fit(positions, axis) else {
                self.set_active(false, commands, clip_materials, cut_materials, visibility);
                return;
            };
            self.rebase(frame, positions, commands, images, meshes, visibility);
            self.stock_revision = Some(inputs.stock_revision);
            self.base_time = base_time;
            self.reset_id += 1;
            self.cursor = cursor.time_seconds;
        } else if cursor.time_seconds < self.cursor - 1e-9 {
            self.reset_id += 1;
        }
        self.cursor = cursor.time_seconds;
        let frame = self.frame.expect("rebased above");
        let mut tools = Vec::new();
        let traveled = traveled_segments(
            inputs.lines,
            cursor.path_id,
            self.base_time,
            cursor.time_seconds,
            &frame,
            |time| self.tool_index(time, &mut tools),
        );
        let segments: Vec<GpuSegment> = traveled.into_iter().map(|(_, s)| s).collect();
        let finite_flutes = segments.iter().all(|segment| {
            segment.a.is_finite()
                && segment.b.is_finite()
                && segment.a.w >= 0.0
                && segment.a.w.fract() == 0.0
                && tools.get(segment.a.w as usize).is_some_and(|geometry| {
                    flute::covers_stock(
                        frame.top,
                        [segment.a.z, segment.b.z],
                        geometry.flute_length,
                    )
                })
        });
        let profiles: Option<Vec<GpuProfile>> = tools.into_iter().map(profile_table).collect();
        let Some(profiles) = profiles.filter(|_| finite_flutes) else {
            self.set_active(false, commands, clip_materials, cut_materials, visibility);
            return;
        };
        self.set_active(true, commands, clip_materials, cut_materials, visibility);
        if rebased {
            self.apply_frame(true, clip_materials, cut_materials);
        }
        if stamp.reset_id != self.reset_id
            || stamp.segments != segments
            || stamp.profiles != profiles
            || stamp.field.as_ref() != Some(&self.field)
        {
            *stamp = GpuStockStamp {
                field: Some(self.field.clone()),
                dims: frame.dims,
                min: frame.min,
                texel: frame.texel,
                reset_id: self.reset_id,
                revision: stamp.revision.wrapping_add(1),
                segments,
                profiles,
            };
        }
    }

    fn rebase(
        &mut self,
        frame: Frame,
        positions: &[f32],
        commands: &mut Commands,
        images: &mut Assets<Image>,
        meshes: &mut Assets<Mesh>,
        visibility: &mut Query<&mut Visibility>,
    ) {
        let size = Extent3d {
            width: frame.dims.x,
            height: frame.dims.y,
            depth_or_array_layers: 1,
        };
        if self.frame.is_none_or(|old| old.dims != frame.dims) {
            let mut field = Image::new(
                size,
                TextureDimension::D2,
                UNCUT
                    .to_le_bytes()
                    .repeat((frame.dims.x * frame.dims.y) as usize),
                TextureFormat::R32Float,
                RenderAssetUsages::RENDER_WORLD,
            );
            field.texture_descriptor.usage = TextureUsages::STORAGE_BINDING
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST;
            self.field = images.add(field);
            if let Some((entity, _)) = self.grid.take() {
                commands.entity(entity).despawn();
            }
        }
        let columns = rasterize_columns(positions, &frame);
        let bytes = columns
            .iter()
            .flat_map(|[top, bottom]| top.to_le_bytes().into_iter().chain(bottom.to_le_bytes()))
            .collect();
        self.columns = images.add(Image::new(
            size,
            TextureDimension::D2,
            bytes,
            TextureFormat::Rg32Float,
            RenderAssetUsages::RENDER_WORLD,
        ));
        if self.grid.is_none() {
            let entity = commands
                .spawn((
                    Name::new("Native CAM GPU-cut stock surface"),
                    Mesh3d(meshes.add(grid_mesh(frame.dims))),
                    MeshMaterial3d(self.cut.clone()),
                    Transform::IDENTITY,
                    NoFrustumCulling,
                    NotShadowCaster,
                    NotShadowReceiver,
                    Visibility::Hidden,
                ))
                .id();
            self.grid = Some((entity, frame.dims));
        } else if let Some((entity, _)) = self.grid {
            if let Ok(mut current) = visibility.get_mut(entity) {
                *current = Visibility::Hidden;
            }
        }
        self.frame = Some(frame);
    }

    fn apply_frame(
        &self,
        active: bool,
        clip_materials: &mut Assets<StockClipMaterial>,
        cut_materials: &mut Assets<CutSurfaceMaterial>,
    ) {
        let Some(frame) = self.frame else {
            return;
        };
        let uniform = frame.uniform(active);
        if let Some(mut material) = clip_materials.get_mut(&self.clip) {
            material.extension.frame = uniform;
            material.extension.field = self.field.clone();
            material.extension.columns = self.columns.clone();
        }
        if let Some(mut material) = cut_materials.get_mut(&self.cut) {
            material.extension.frame = uniform;
            material.extension.field = self.field.clone();
            material.extension.columns = self.columns.clone();
        }
    }

    fn set_active(
        &mut self,
        active: bool,
        commands: &mut Commands,
        clip_materials: &mut Assets<StockClipMaterial>,
        cut_materials: &mut Assets<CutSurfaceMaterial>,
        visibility: &mut Query<&mut Visibility>,
    ) {
        if let Some((entity, _)) = self.grid {
            visibility::set_grid(entity, active, commands, visibility);
        }
        if self.active == active {
            return;
        }
        self.active = active;
        if !active {
            self.stock_revision = None;
        }
        self.apply_frame(active, clip_materials, cut_materials);
    }
}

/// One vertex per field texel; positions carry texel indices for the
/// vertex shader, which places them on the cut surface.
fn grid_mesh(dims: UVec2) -> Mesh {
    let (width, height) = (dims.x, dims.y);
    let positions: Vec<[f32; 3]> = (0..height)
        .flat_map(|y| (0..width).map(move |x| [x as f32, y as f32, 0.0]))
        .collect();
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    let mut indices = Vec::with_capacity(((width - 1) * (height - 1) * 6) as usize);
    for y in 0..height.saturating_sub(1) {
        for x in 0..width.saturating_sub(1) {
            let i = x + y * width;
            indices.extend([i, i + 1, i + width + 1, i, i + width + 1, i + width]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[derive(Clone, Copy, ShaderType)]
struct StampParams {
    rect_min: UVec2,
    rect_size: UVec2,
    grid_min: Vec2,
    texel: f32,
    count: u32,
}

#[derive(Resource)]
struct StampPipeline {
    layout: BindGroupLayoutDescriptor,
    clear: CachedComputePipelineId,
    stamp: CachedComputePipelineId,
}

#[derive(Resource, Default)]
struct StampProgress {
    reset_id: u64,
    revision: u64,
    stamped: usize,
    field: Option<AssetId<Image>>,
}

fn init_stamp_pipeline(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Option<Res<StampPipeline>>,
    stamp: Option<Res<GpuStockStamp>>,
) {
    // Empty CAD startup does not use stock removal. Queue its compute shaders
    // only when playback supplies a field, before this frame's pipeline queue.
    if pipeline.is_some() || stamp.is_none_or(|stamp| stamp.field.is_none()) {
        return;
    }
    let layout = BindGroupLayoutDescriptor::new(
        "limo-cad GPU stock stamp",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_storage_2d(TextureFormat::R32Float, StorageTextureAccess::ReadWrite),
                uniform_buffer::<StampParams>(false),
                storage_buffer_read_only::<GpuSegment>(false),
                storage_buffer_read_only::<GpuProfile>(false),
            ),
        ),
    );
    let pipeline = |entry: &'static str| {
        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("limo-cad GPU stock {entry}").into()),
            layout: vec![layout.clone()],
            shader: STAMP_SHADER,
            entry_point: Some(Cow::from(entry)),
            ..default()
        })
    };
    let (clear, stamp) = (pipeline("clear"), pipeline("stamp"));
    commands.insert_resource(StampPipeline {
        layout,
        clear,
        stamp,
    });
}

/// Texel rectangle covered by the given segments' cutters.
fn stamp_rect(stamp: &GpuStockStamp, segments: &[GpuSegment]) -> Option<(UVec2, UVec2)> {
    let mut low = Vec2::splat(f32::INFINITY);
    let mut high = Vec2::splat(f32::NEG_INFINITY);
    for segment in segments {
        let radius = stamp
            .profiles
            .get(segment.a.w as usize)
            .map_or(0.0, |profile| profile.radius);
        low = low.min(
            segment
                .a
                .truncate()
                .truncate()
                .min(segment.b.truncate().truncate())
                - radius,
        );
        high = high.max(
            segment
                .a
                .truncate()
                .truncate()
                .max(segment.b.truncate().truncate())
                + radius,
        );
    }
    let to_texel = |p: Vec2| (p - stamp.min) / stamp.texel;
    let first = to_texel(low).floor().max(Vec2::ZERO).as_uvec2();
    let last = to_texel(high)
        .ceil()
        .min(stamp.dims.as_vec2())
        .max(Vec2::ZERO)
        .as_uvec2();
    (last.x > first.x && last.y > first.y).then(|| (first, last - first))
}

#[allow(clippy::too_many_arguments)]
fn stamp_field(
    mut render_context: RenderContext,
    stamp: Option<Res<GpuStockStamp>>,
    mut progress: ResMut<StampProgress>,
    pipeline: Option<Res<StampPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let (Some(stamp), Some(pipeline)) = (stamp, pipeline) else {
        return;
    };
    let Some(field) = stamp.field.as_ref() else {
        return;
    };
    let fresh = progress.field != Some(field.id()) || progress.reset_id != stamp.reset_id;
    if !fresh && progress.revision == stamp.revision {
        return;
    }
    let (Some(clear), Some(stamp_pipeline), Some(image)) = (
        pipeline_cache.get_compute_pipeline(pipeline.clear),
        pipeline_cache.get_compute_pipeline(pipeline.stamp),
        images.get(field),
    ) else {
        return;
    };
    let from = if fresh {
        0
    } else {
        progress.stamped.saturating_sub(1)
    };
    let layout = pipeline_cache.get_bind_group_layout(&pipeline.layout);
    let dispatch = |encoder: &mut bevy::render::render_resource::CommandEncoder,
                    compute: &bevy::render::render_resource::ComputePipeline,
                    rect: (UVec2, UVec2),
                    segments: &[GpuSegment]| {
        let mut params = UniformBuffer::from(StampParams {
            rect_min: rect.0,
            rect_size: rect.1,
            grid_min: stamp.min,
            texel: stamp.texel,
            count: segments.len() as u32,
        });
        params.write_buffer(&device, &queue);
        let mut segment_buffer = StorageBuffer::from(if segments.is_empty() {
            vec![GpuSegment::default()]
        } else {
            segments.to_vec()
        });
        segment_buffer.write_buffer(&device, &queue);
        let mut profile_buffer = StorageBuffer::from(if stamp.profiles.is_empty() {
            vec![GpuProfile {
                radius: 0.0,
                _pad0: 0.0,
                _pad1: 0.0,
                _pad2: 0.0,
                heights: [Vec4::ZERO; PROFILE_SAMPLES / 4],
            }]
        } else {
            stamp.profiles.clone()
        });
        profile_buffer.write_buffer(&device, &queue);
        let bind_group = device.create_bind_group(
            Some("limo-cad GPU stock stamp"),
            &layout,
            &BindGroupEntries::sequential((
                &image.texture_view,
                params.binding().unwrap(),
                segment_buffer.binding().unwrap(),
                profile_buffer.binding().unwrap(),
            )),
        );
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("limo-cad GPU stock stamp"),
            ..default()
        });
        pass.set_pipeline(compute);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(
            rect.1.x.div_ceil(WORKGROUP),
            rect.1.y.div_ceil(WORKGROUP),
            1,
        );
    };
    let encoder = render_context.command_encoder();
    if fresh {
        dispatch(encoder, clear, (UVec2::ZERO, stamp.dims), &[]);
    }
    let todo = &stamp.segments[from.min(stamp.segments.len())..];
    if let Some(rect) = stamp_rect(&stamp, todo) {
        dispatch(encoder, stamp_pipeline, rect, todo);
    }
    *progress = StampProgress {
        reset_id: stamp.reset_id,
        revision: stamp.revision,
        stamped: stamp.segments.len(),
        field: Some(field.id()),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_cam::CamToolKind;

    #[test]
    fn closing_stock_retires_the_grid_and_all_document_texture_handles() {
        use bevy_ecs::system::SystemState;
        let mut world = World::new();
        let mut system = SystemState::<(Commands, Query<&mut Visibility>)>::new(&mut world);
        let mut images = Assets::<Image>::default();
        let mut meshes = Assets::<Mesh>::default();
        let mut clip = Assets::<StockClipMaterial>::default();
        let mut cut = Assets::<CutSurfaceMaterial>::default();
        let mut stock = GpuStock::new(
            &mut images,
            &mut clip,
            &mut cut,
            StandardMaterial::default(),
        );
        let mut stamp = GpuStockStamp::default();
        let positions = square(20., 5.);
        let lines = [ViewportLineLayer {
            segments: vec![0., 10., 4., 20., 10., 4.].into(),
            playback: Some(super::super::ViewportLinePlayback {
                path_id: 1,
                completed_color: [1.; 4],
                segment_times: vec![0., 1.].into(),
                single_tool: true,
                removes_stock: true,
            }),
            ..default()
        }];
        {
            let (mut commands, mut visibility) = system.get_mut(&mut world).unwrap();
            stock.update(
                GpuStockInputs {
                    enabled: true,
                    stock_positions: Some(&positions),
                    stock_time: Some(0.),
                    stock_revision: 1,
                    cursor: Some(ViewportCamPathProgress {
                        path_id: 1,
                        time_seconds: 0.5,
                        position: [10., 10., 4.],
                    }),
                    tool: Some(ViewportCamTool {
                        tip: [10., 10., 4.],
                        axis: [0., 0., 1.],
                        geometry: geometry(CamToolKind::FlatEndMill, None),
                    }),
                    lines: &lines,
                },
                &mut commands,
                &mut images,
                &mut meshes,
                &mut clip,
                &mut cut,
                &mut stamp,
                &mut visibility,
            );
        }
        system.apply(&mut world);
        let grid = stock.grid.unwrap().0;
        let field = stock.field.id();
        let columns = stock.columns.id();
        assert!(stock.active);
        assert_eq!(stamp.field.as_ref().unwrap().id(), field);
        {
            let (mut commands, mut visibility) = system.get_mut(&mut world).unwrap();
            stock.update(
                GpuStockInputs {
                    enabled: false,
                    stock_positions: None,
                    stock_time: None,
                    stock_revision: 2,
                    cursor: None,
                    tool: None,
                    lines: &[],
                },
                &mut commands,
                &mut images,
                &mut meshes,
                &mut clip,
                &mut cut,
                &mut stamp,
                &mut visibility,
            );
        }
        system.apply(&mut world);
        assert!(world.get_entity(grid).is_err());
        assert!(stock.frame.is_none() && stock.grid.is_none() && !stock.active);
        assert!(stamp.field.is_none() && stamp.segments.is_empty() && stamp.profiles.is_empty());
        assert_ne!(stock.field.id(), field);
        assert_ne!(stock.columns.id(), columns);
        assert_eq!(clip.get(&stock.clip).unwrap().extension.field, stock.field);
        assert_eq!(
            cut.get(&stock.cut).unwrap().extension.columns,
            stock.columns
        );
    }

    #[test]
    fn unrepresentable_frames_fall_back_to_cpu_stock() {
        assert!(Frame::fit(&square(20., UNCUT), Vec3::Z).is_none());
        assert!(Frame::fit(&square(f32::from_bits(1), 1.), Vec3::Z).is_none());
        let mut points = square(20., 5.);
        points[0] = f32::NAN;
        assert!(Frame::fit(&points, Vec3::Z).is_none());
    }

    fn geometry(kind: CamToolKind, corner: Option<f64>) -> CamCutterGeometryDto {
        CamCutterGeometryDto {
            kind,
            diameter: 10.0,
            flute_length: 20.0,
            overall_length: 50.0,
            point_angle_degrees: None,
            corner_radius: corner,
            corner_chamfer: None,
        }
    }

    fn sample(profile: &GpuProfile, i: usize) -> f32 {
        profile.heights[i / 4][i % 4]
    }

    #[test]
    fn profile_tables_follow_the_analytic_cutter() {
        let flat = profile_table(geometry(CamToolKind::FlatEndMill, None)).unwrap();
        assert!((0..PROFILE_SAMPLES).all(|i| sample(&flat, i) == 0.0));
        assert_eq!(flat.radius, 5.0);
        let ball = profile_table(geometry(CamToolKind::BallEndMill, None)).unwrap();
        for i in [0, 20, 40, 63] {
            let radial = 5.0 * i as f32 / 63.0;
            let expected = 5.0 - (25.0 - radial * radial).max(0.0).sqrt();
            assert!((sample(&ball, i) - expected).abs() < 1e-3, "{i}");
        }
        let bull = profile_table(geometry(CamToolKind::BullNoseEndMill, Some(1.0))).unwrap();
        assert_eq!(sample(&bull, 0), 0.0);
        assert!(
            (sample(&bull, 50) - 0.0).abs() < 1e-6,
            "inside the 4 mm flat land"
        );
        assert!(
            (sample(&bull, 63) - 1.0).abs() < 1e-3,
            "full radius at the corner height"
        );
    }

    fn square(size: f32, height: f32) -> Vec<f32> {
        let quad = |z: f32| {
            [
                [0.0, 0.0, z],
                [size, 0.0, z],
                [size, size, z],
                [0.0, 0.0, z],
                [size, size, z],
                [0.0, size, z],
            ]
        };
        quad(height)
            .into_iter()
            .chain(quad(0.0))
            .flatten()
            .collect()
    }

    #[test]
    fn columns_cover_the_stock_footprint_only() {
        let positions = square(20.0, 5.0);
        let frame = Frame::fit(&positions, Vec3::Z).unwrap();
        assert!(frame.dims.x <= MAX_FIELD_TEXELS + 2);
        let columns = rasterize_columns(&positions, &frame);
        let at = |p: Vec2| {
            let local = frame.local(p.extend(0.0)).truncate();
            let t = ((local - frame.min) / frame.texel).as_uvec2();
            columns[(t.x + t.y * frame.dims.x) as usize]
        };
        let inside = at(Vec2::new(10.0, 10.0));
        assert!((inside[0] - frame.local(Vec3::new(10.0, 10.0, 5.0)).z).abs() < 1e-4);
        assert!((inside[1] - frame.local(Vec3::new(10.0, 10.0, 0.0)).z).abs() < 1e-4);
        let outside = columns[0];
        assert!(outside[0] < outside[1], "the margin texel holds no stock");
    }

    #[test]
    fn traveled_segments_clip_to_the_retained_frame_and_cursor() {
        let positions = square(20.0, 5.0);
        let frame = Frame::fit(&positions, Vec3::Z).unwrap();
        let layer = ViewportLineLayer {
            segments: vec![
                0.0, 10.0, 4.0, 20.0, 10.0, 4.0, 20.0, 10.0, 4.0, 20.0, 0.0, 4.0,
            ]
            .into(),
            playback: Some(super::super::ViewportLinePlayback {
                path_id: 3,
                completed_color: [1.0; 4],
                segment_times: vec![0.0, 2.0, 2.0, 3.0].into(),
                single_tool: true,
                removes_stock: true,
            }),
            ..default()
        };
        let lines = [layer];
        let traveled = traveled_segments(&lines, 3, 1.0, 2.5, &frame, |_| Some(0));
        assert_eq!(traveled.len(), 2);
        let world = |p: Vec4| frame.origin + frame.x * p.x + frame.y * p.y + frame.z * p.z;
        assert!(world(traveled[0].1.a).distance(Vec3::new(10.0, 10.0, 4.0)) < 1e-4);
        assert!(world(traveled[0].1.b).distance(Vec3::new(20.0, 10.0, 4.0)) < 1e-4);
        assert!(world(traveled[1].1.b).distance(Vec3::new(20.0, 5.0, 4.0)) < 1e-4);
        assert!(traveled_segments(&lines, 4, 1.0, 2.5, &frame, |_| Some(0)).is_empty());
        assert!(traveled_segments(&lines, 3, 2.5, 2.5, &frame, |_| Some(0)).is_empty());
        let mut rapid = lines;
        rapid[0].playback.as_mut().unwrap().removes_stock = false;
        assert!(traveled_segments(&rapid, 3, 1., 2.5, &frame, |_| Some(0)).is_empty());
    }

    #[test]
    fn tools_are_attributed_by_the_time_they_were_observed() {
        let mut images = Assets::<Image>::default();
        let mut clip = Assets::<StockClipMaterial>::default();
        let mut cut = Assets::<CutSurfaceMaterial>::default();
        let mut stock = GpuStock::new(
            &mut images,
            &mut clip,
            &mut cut,
            StandardMaterial::default(),
        );
        let (a, b) = (
            geometry(CamToolKind::FlatEndMill, None),
            geometry(CamToolKind::BallEndMill, None),
        );
        stock.observe_tool(1.0, a);
        stock.observe_tool(2.0, a);
        stock.observe_tool(5.0, b);
        assert_eq!(stock.tools.len(), 2);
        let mut tools = Vec::new();
        assert_eq!(
            stock.tool_index(0.5, &mut tools),
            Some(0),
            "before any observation"
        );
        assert_eq!(stock.tool_index(4.0, &mut tools), Some(0));
        assert_eq!(stock.tool_index(6.0, &mut tools), Some(1));
        assert_eq!(tools, vec![a, b]);
    }
}
