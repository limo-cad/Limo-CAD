//! Sheet paper for the native drawing workspace. Projected edges, notes, and
//! view names share one layout: paper millimetres, origin at the upper left.
use super::*;
use bevy::ui::UiTransform;
use limo_cad_occt::DrawingProjectionDto;
use limo_cad_sketch::{DrawingSheetDto, DrawingViewDto};
#[path = "drawing_annotations.rs"]
mod annotations;
#[path = "drawing_dimensions.rs"]
mod dimensions;
#[path = "drawing_edges.rs"]
mod edges;
#[path = "drawing_frame.rs"]
mod frame;
#[path = "drawing_paper_view.rs"]
mod view;
pub(super) use annotations::resolved_center_circle;
pub(super) use annotations::{chamfer_caption, valid_line_dimension, valid_point_line};
pub(super) use view::diagnostics::snapshot as diagnostics;
pub(super) use view::navigation_snapshot;
pub(super) use view::{canvas, paint, repaint, PaperView};

type PaperPlane = Option<(f64, [f64; 2], [f64; 2], [f64; 2], [f64; 2])>;

pub(super) fn evict_document_geometry(world: &mut World, owner: &DocumentContext) {
    if let Some(mut cache) = world.get_resource_mut::<edges::EdgeCache>() {
        cache.evict_document(owner);
    }
    let Some(mut state) = world.get_resource_mut::<Workbench>() else {
        return;
    };
    if !state
        .paper_view
        .as_ref()
        .is_some_and(|view| view.source.belongs_to_document(owner))
    {
        return;
    }
    let image = state.widgets.entity("drawing-projected-edges");
    state.paper_document = None;
    state.paper_key = None;
    state.paper.clear();
    state.paper_labels.clear();
    state.paper_fills.clear();
    if let Some(view) = &mut state.paper_view {
        view.marks.clear();
        view.art_context = None;
    }

    if let Some(entity) = image {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            entity.remove::<ImageNode>();
        }
    }
    world.remove_resource::<FrameCache>();
}

pub(super) fn advance_sheet_selection(
    world: &mut World,
    owner: &DocumentContext,
    from: u64,
    to: u64,
) {
    if let Some(mut cache) = world.get_resource_mut::<edges::EdgeCache>() {
        cache.advance_sheet_selection(owner, from, to);
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct ProjectionStamp(edges::SourceKey);
pub(super) fn projection_stamp(state: &Workbench) -> Option<ProjectionStamp> {
    state.paper_key.as_ref()?;
    Some(ProjectionStamp(state.paper_view.as_ref()?.source.clone()))
}
pub(super) fn same_projection(state: &Workbench, stamp: &ProjectionStamp) -> bool {
    state.paper_key.is_some()
        && state
            .paper_view
            .as_ref()
            .is_some_and(|view| view.source == stamp.0)
}

pub(super) fn transform(state: &Workbench) -> Option<super::drawing_navigation::PaperTransform> {
    state.paper_key.as_ref()?;
    state
        .paper_view
        .as_ref()
        .map(|view| view.navigation.transform())
}
pub(super) fn with_projections<T>(
    world: &World,
    state: &Workbench,
    read: impl FnOnce(&edges::Projections, &edges::ProjectionBases) -> T,
) -> Option<T> {
    state.paper_key.as_ref()?;
    let view = state.paper_view.as_ref()?;
    let cache = world.get_resource::<edges::EdgeCache>()?;
    Some(read(
        cache.projections(&view.source)?,
        cache.bases(&view.source)?,
    ))
}

#[derive(Clone)]
pub(super) struct AnnotationMark {
    pub id: u64,
    pub part: usize,
    pub center: [f64; 2],
    pub size: [f64; 2],
    pub angle: f32,
    pub linear_points: Option<[[f64; 2]; 2]>,
    pub radial: Option<RadialDrag>,
    pub angular: Option<AngularDrag>,
    pub ordinate_points: Option<[[f64; 2]; 2]>,
    pub position_resolved: bool,
}
#[derive(Clone, Copy)]
pub(super) struct RadialDrag {
    pub center: [f64; 2],
    pub paper_radius: f64,
    pub shoulder: [f64; 2],
}
#[derive(Clone, Copy)]
pub(super) struct AngularDrag {
    pub vertex: [f64; 2],
    pub text: [f64; 2],
}
pub(super) fn annotation_marks(state: &Workbench) -> &[AnnotationMark] {
    if state.paper_key.is_none() {
        return &[];
    }
    state
        .paper_view
        .as_ref()
        .map_or(&[], |v| v.marks.as_slice())
}

pub(super) fn annotation_preview(
    world: &mut World,
    state: &mut Workbench,
    sheet: &DrawingSheetDto,
) -> Result<(), String> {
    let Some((revision, units)) = state.paper_view.as_ref().and_then(|v| v.art_context) else {
        return Ok(());
    };
    let result = (|| {
        let view = state.paper_view.as_mut().ok_or("Open drawing paper")?;
        let cache = world
            .get_resource::<edges::EdgeCache>()
            .ok_or("Drawing projection is not ready")?;
        let projections = cache
            .projections(&view.source)
            .ok_or("Drawing projection changed; wait for the document refresh")?;
        let labels = cache
            .source_labels(&view.source)
            .ok_or("Drawing source marks changed; wait for the document refresh")?;
        view.annotations(sheet, projections, units, labels)
    })();
    match result {
        Ok(art) => {
            view::publish(state, revision, sheet, units, art);
            repaint(world, state)
        }
        Err(error) => {
            view::fail(world, state, &error);
            Err(error)
        }
    }
}

#[derive(Resource, Default)]
struct FrameCache(Option<(frame::Source<'static>, annotations::Art)>);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Ink {
    #[default]
    Drawing,
    Center,
    Revision,
    ViewName,
    Derived,
    Frame,
    FrameText,
    Overflow,
}
impl Ink {
    fn color(self) -> Color {
        match self {
            Self::Drawing => Color::srgb_u8(36, 40, 45),
            Self::Center => Color::srgb_u8(53, 97, 112),
            Self::Revision => Color::srgb_u8(196, 59, 77),
            Self::ViewName => Color::srgb_u8(75, 81, 89),
            Self::Derived => Color::srgb_u8(93, 80, 200),
            Self::Frame => Color::srgb_u8(74, 80, 88),
            Self::FrameText => Color::srgb_u8(48, 52, 58),
            Self::Overflow => Color::srgb_u8(181, 68, 50),
        }
    }
}

#[derive(Clone)]
pub(super) struct Fill {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub round: bool,
}

#[derive(Clone, Default)]
pub(super) struct Label {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub width_mm: f32,
    pub height_mm: f32,
    pub text_height_mm: f32,
    pub mask: bool,
    pub ink: Ink,
    pub align: LabelAlign,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum LabelAlign {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Clone, Copy)]
pub(super) struct Segment {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub hidden: bool,
    pub width_mm: f32,
    pub arrow: bool,
    pub ink: Ink,
}

#[derive(Resource)]
struct ArrowTexture(Handle<Image>);

fn raster_stroke_width(width_mm: f32, paper_scale: f32, render_scale: f32) -> f32 {
    (width_mm * paper_scale).max(0.5).max(render_scale.recip())
}

fn arrow_texture(world: &mut World) -> Handle<Image> {
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    if let Some(texture) = world.get_resource::<ArrowTexture>() {
        return texture.0.clone();
    }
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><polygon points="0,32 64,7.68 64,56.32" fill="white"/></svg>"#;
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).unwrap();
    let mut pixels = resvg::tiny_skia::Pixmap::new(64, 64).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    let rgba = pixels
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let pixel = pixel.demultiply();
            [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
        })
        .collect();
    let image = Image::new(
        Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let handle = world.resource_mut::<Assets<Image>>().add(image);
    world.insert_resource(ArrowTexture(handle.clone()));
    handle
}

fn mark_paper_input(world: &mut World, entity: Entity) {
    if world
        .get::<interface_shell::InterfaceCanvasOccluder>(entity)
        .is_none()
    {
        world
            .entity_mut(entity)
            .insert(interface_shell::InterfaceCanvasOccluder("drawing"));
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_primitives(
    world: &mut World,
    camera: Entity,
    widgets: &mut Widgets,
    paper: Entity,
    scale: f32,
    render_scale: f32,
    prefix: &str,
    segments: &[Segment],
    labels: &[Label],
    fills: &[Fill],
    stroke_layer: i32,
    fill_layer: i32,
    text_layer: i32,
) {
    for (index, fill) in fills.iter().enumerate() {
        let key = format!("{prefix}-fill-{index}");
        let mut node = rect(
            fill.x * scale,
            fill.y * scale,
            fill.width * scale,
            fill.height * scale,
        );
        if fill.round {
            node.border_radius = BorderRadius::MAX;
        }
        widgets.panel(world, camera, &key, node, Color::WHITE, fill_layer);
        widgets.parent(world, &key, paper);
        mark_paper_input(world, widgets.entity(&key).unwrap());
    }
    for (index, segment) in segments.iter().enumerate() {
        let (x1, y1) = place(0., 0., scale, segment.x1, segment.y1);
        let (x2, y2) = place(0., 0., scale, segment.x2, segment.y2);
        let delta = Vec2::new(x2 - x1, y2 - y1);
        let length = if segment.arrow {
            delta.length().max(0.5)
        } else {
            delta.length()
        };
        let midpoint = Vec2::new((x1 + x2) * 0.5, (y1 + y2) * 0.5);
        let key = format!("{prefix}-edge-{index}");
        let thickness = if segment.arrow {
            length
        } else {
            raster_stroke_width(segment.width_mm, scale, render_scale)
        };
        widgets.panel(
            world,
            camera,
            &key,
            Node {
                position_type: PositionType::Absolute,
                left: px(midpoint.x - length * 0.5),
                top: px(midpoint.y - thickness * 0.5),
                width: px(length),
                height: px(thickness),
                ..default()
            },
            if segment.arrow {
                Color::NONE
            } else if segment.hidden {
                Color::srgb_u8(132, 138, 146)
            } else {
                segment.ink.color()
            },
            stroke_layer,
        );
        widgets.parent(world, &key, paper);
        if let Some(entity) = widgets.entity(&key) {
            mark_paper_input(world, entity);
            if segment.arrow {
                let texture = arrow_texture(world);
                world.entity_mut(entity).insert(ImageNode {
                    image: texture,
                    color: segment.ink.color(),
                    ..default()
                });
            } else {
                world.entity_mut(entity).remove::<ImageNode>();
            }
            world
                .entity_mut(entity)
                .insert(UiTransform::from_rotation(Rot2::radians(
                    delta.y.atan2(delta.x),
                )));
        }
    }
    for (index, label) in labels.iter().enumerate() {
        let box_key = format!("{prefix}-label-box-{index}");
        widgets.panel(
            world,
            camera,
            &box_key,
            label_box(label, scale),
            if label.mask {
                Color::WHITE
            } else {
                Color::NONE
            },
            text_layer,
        );
        widgets.parent(world, &box_key, paper);
        let label_box = widgets.entity(&box_key).unwrap();
        mark_paper_input(world, label_box);
        world
            .entity_mut(label_box)
            .insert(UiTransform::from_rotation(Rot2::radians(label.angle)));
        widgets.text(
            world,
            camera,
            &format!("{prefix}-dimension-{index}"),
            intrinsic_label_node(),
            &label.text,
            label.text_height_mm * scale,
            text_layer,
        );
        let key = format!("{prefix}-dimension-{index}");
        widgets.parent(world, &key, label_box);
        world.entity_mut(widgets.entity(&key).unwrap()).insert((
            TextColor(label.ink.color()),
            TextLayout::new(Justify::Left, bevy::text::LineBreak::NoWrap),
            BackgroundColor(if label.mask {
                Color::WHITE
            } else {
                Color::NONE
            }),
            UiTransform::default(),
        ));
        world
            .get_mut::<TextFont>(widgets.entity(&key).unwrap())
            .unwrap()
            .font_size = bevy::text::FontSize::Px(label.text_height_mm * scale);
    }
}

fn label_box(label: &Label, scale: f32) -> Node {
    Node {
        justify_content: match label.align {
            LabelAlign::Start => JustifyContent::Start,
            LabelAlign::Center => JustifyContent::Center,
            LabelAlign::End => JustifyContent::End,
        },
        align_items: AlignItems::Center,
        overflow: Overflow::visible(),
        ..rect(
            (label.x - label.width_mm * 0.5) * scale,
            (label.y - label.height_mm * 0.5) * scale,
            label.width_mm * scale,
            label.height_mm * scale,
        )
    }
}
fn intrinsic_label_node() -> Node {
    Node {
        flex_shrink: 0.,
        ..default()
    }
}

#[cfg(test)]
fn sheet_layout(width: f32, height: f32, side: f32, sheet_w: f32, sheet_h: f32) -> (f32, f32, f32) {
    let scale = ((width - side - 32.).max(1.) / sheet_w).min((height - 232.).max(1.) / sheet_h);
    (side + (width - side - sheet_w * scale) * 0.5, 132., scale)
}

fn place(origin_x: f32, origin_y: f32, scale: f32, x: f32, y: f32) -> (f32, f32) {
    (origin_x + x * scale, origin_y + y * scale)
}

fn view_name_label(
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    size: f64,
    center_bottom: Option<f64>,
) -> Label {
    let height = (projection.bounds[3] - projection.bounds[1]).abs() * view.scale;
    let reserved = view.position[1]
        + height * 0.5
        + limo_cad_occt::drawing_presentation::layout::DIMENSION_OFFSET_MM;
    let dimension_ink = limo_cad_occt::drawing_presentation::layout::dimension_ink_y(
        reserved, reserved, 0.25, true,
    );
    let baseline = limo_cad_occt::drawing_presentation::layout::view_caption_baseline(
        view.position[1],
        height,
        size,
        Some(dimension_ink),
    );
    let baseline = limo_cad_occt::drawing_presentation::centers::caption_baseline(
        baseline,
        size,
        center_bottom,
    );
    let scale = if view.scale >= 1. {
        format!("{}:1", view.scale)
    } else {
        format!("1:{}", (100. / view.scale).round() / 100.)
    };
    let text = format!("{} · {scale}", view.name);
    Label {
        width_mm: (text.chars().count() as f64 * size * 0.7 + 2.2) as f32,
        text,
        x: view.position[0] as f32,
        y: (baseline - size * 0.4) as f32,
        height_mm: (size * 1.18 + 1.5) as f32,
        text_height_mm: size as f32,
        ink: Ink::ViewName,
        ..default()
    }
}

/// Measure in model millimetres while placing the lines in paper millimetres.
fn dimension_span(
    mode: limo_cad_sketch::DrawingLinearDimensionMode,
    first: [f64; 2],
    second: [f64; 2],
    offset: f64,
    view_scale: f64,
) -> PaperPlane {
    let g = limo_cad_occt::drawing_presentation::geometry::dimension_span(
        mode, first, second, offset, view_scale,
    )?;
    Some((g.value, g.first, g.second, g.start, g.end))
}

/// Same placement as the drawing export: the view position is the projected
/// bounds center, and paper Y grows downward.
pub(super) fn paper_point(
    view: &DrawingViewDto,
    point: [f64; 2],
    projection: &DrawingProjectionDto,
) -> [f64; 2] {
    let bounds = projection.bounds;
    [
        view.position[0] + (point[0] - (bounds[0] + bounds[2]) * 0.5) * view.scale,
        view.position[1] - (point[1] - (bounds[1] + bounds[3]) * 0.5) * view.scale,
    ]
}

fn sheet_size(sheet: &DrawingSheetDto) -> (f32, f32) {
    let (short, long) = match sheet.format {
        limo_cad_sketch::DrawingSheetFormat::A0 => (841., 1189.),
        limo_cad_sketch::DrawingSheetFormat::A1 => (594., 841.),
        limo_cad_sketch::DrawingSheetFormat::A2 => (420., 594.),
        limo_cad_sketch::DrawingSheetFormat::A3 => (297., 420.),
        limo_cad_sketch::DrawingSheetFormat::A4 => (210., 297.),
        limo_cad_sketch::DrawingSheetFormat::Letter => (215.9, 279.4),
        limo_cad_sketch::DrawingSheetFormat::AnsiB => (279.4, 431.8),
        limo_cad_sketch::DrawingSheetFormat::AnsiC => (431.8, 558.8),
        limo_cad_sketch::DrawingSheetFormat::AnsiD => (558.8, 863.6),
        limo_cad_sketch::DrawingSheetFormat::AnsiE => (863.6, 1117.6),
    };
    match sheet.orientation {
        limo_cad_sketch::DrawingSheetOrientation::Landscape => (long, short),
        limo_cad_sketch::DrawingSheetOrientation::Portrait => (short, long),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_sketch::DrawingViewKind;

    #[test]
    fn intrinsic_label_bounds_stay_centered_when_font_width_differs_from_paper_estimate() {
        use bevy::ui::{ui_layout_system, ui_surface::UiSurface, ContentSize};
        let mut world = World::new();
        world.init_resource::<UiSurface>();
        world.init_resource::<bevy::text::FontCx>();
        world.init_resource::<bevy::text::RemSize>();
        let mut labels = Vec::new();
        for scale in [1., 2.] {
            for (estimate, actual) in [(100., 45.), (30., 75.)] {
                for align in [LabelAlign::Start, LabelAlign::Center, LabelAlign::End] {
                    let label = Label {
                        x: 160.,
                        y: 70.,
                        width_mm: estimate,
                        height_mm: 20.,
                        align,
                        ..default()
                    };
                    let parent = world
                        .spawn((
                            label_box(&label, scale),
                            bevy::ui::LayoutConfig {
                                use_rounding: false,
                            },
                        ))
                        .id();
                    let child = world
                        .spawn((
                            intrinsic_label_node(),
                            ContentSize::fixed_size(Vec2::new(actual * scale, 12. * scale)),
                            ChildOf(parent),
                        ))
                        .id();
                    labels.push((
                        parent,
                        child,
                        estimate * scale,
                        actual * scale,
                        20. * scale,
                        align,
                    ));
                }
            }
        }
        world.run_system_cached(ui_layout_system).unwrap();
        for (parent, child, box_width, text_width, box_height, align) in labels {
            let mut surface = world.resource_mut::<UiSurface>();
            let layout = surface.get_layout(child, false).unwrap().0;
            assert_eq!(
                layout.size.width, text_width,
                "Text must retain its intrinsic width"
            );
            let expected_x = match align {
                LabelAlign::Start => 0.,
                LabelAlign::Center => (box_width - text_width) * 0.5,
                LabelAlign::End => box_width - text_width,
            };
            assert!(
                (layout.location.x - expected_x).abs() < 0.01,
                "{align:?}: box {box_width}x{box_height}, intrinsic {text_width}, actual {layout:?}, expected x {expected_x}"
            );
            assert!(
                (layout.location.y + layout.size.height * 0.5 - box_height * 0.5).abs() < 0.01,
                "{align:?}: box {box_width}x{box_height}, intrinsic {text_width}, actual {layout:?}"
            );
            assert_eq!(
                world.get::<Node>(parent).unwrap().overflow,
                Overflow::visible()
            );
        }
    }

    #[test]
    fn short_custom_dash_keeps_exact_longitudinal_length_in_actual_ui_layout() {
        use bevy::ui::{ui_layout_system, ui_surface::UiSurface, LayoutConfig};
        let mut world = World::new();
        world.init_resource::<UiSurface>();
        world.init_resource::<bevy::text::FontCx>();
        world.init_resource::<bevy::text::RemSize>();
        let camera = world.spawn_empty().id();
        let mut widgets = Widgets::default();
        let mut strokes = Vec::new();
        for (i, scale) in [0.5, 1., 2.].into_iter().enumerate() {
            let paper = world
                .spawn((
                    rect(0., 0., 300., 200.),
                    LayoutConfig {
                        use_rounding: false,
                    },
                ))
                .id();
            for (j, ink) in [Ink::Frame, Ink::Drawing, Ink::Center]
                .into_iter()
                .enumerate()
            {
                let segment = Segment {
                    x1: 10.,
                    y1: 10. + j as f32,
                    x2: 10.01,
                    y2: 10. + j as f32,
                    hidden: false,
                    width_mm: 0.25,
                    arrow: false,
                    ink,
                };
                let key = format!("tiny-dash-{i}-{j}");
                paint_primitives(
                    &mut world,
                    camera,
                    &mut widgets,
                    paper,
                    scale,
                    2.,
                    &key,
                    &[segment],
                    &[],
                    &[],
                    9,
                    10,
                    11,
                );
                let entity = widgets.entity(&format!("{key}-edge-0")).unwrap();
                world.entity_mut(entity).remove::<UiTargetCamera>();
                strokes.push((entity, (10.01_f32 * scale - 10. * scale).abs()));
            }
        }
        world.run_system_cached(ui_layout_system).unwrap();
        for (entity, expected) in strokes {
            let size = world.get::<ComputedNode>(entity).unwrap().size();
            assert!(
                (size.x - expected).abs() < 1e-6,
                "Short dash was lengthened: {size:?}, expected {expected}"
            );
            assert!(size.x < 0.5);
            assert!(size.y >= 0.5, "Existing physical-width floor remains");
        }
    }

    #[test]
    fn paper_strokes_cover_pixels_at_fractional_positions_and_dpi() {
        fn covers_pixel(center: f32, width: f32) -> bool {
            let left = f64::from(center) - f64::from(width) * 0.5;
            let right = f64::from(center) + f64::from(width) * 0.5;
            (left - 0.5).ceil() + 0.5 <= right
        }
        let (x, _, paper_scale) = sheet_layout(1360., 860., 240., 297., 210.);
        let top_right = x + 100. * paper_scale;
        assert!(!covers_pixel(top_right, 0.25 * paper_scale));
        for dpi in [1., 1.25, 1.5, 2.] {
            for ui_scale in [0.75, 1., 1.5] {
                let render_scale = dpi * ui_scale;
                let width = raster_stroke_width(0.25, paper_scale, render_scale);
                assert!(width * render_scale >= 1.);
                assert!(covers_pixel(top_right * render_scale, width * render_scale));
                assert_eq!(
                    raster_stroke_width(2., paper_scale, render_scale),
                    2. * paper_scale
                );
            }
        }
    }

    #[test]
    fn paper_subpixel_extensions_survive_actual_ui_layout() {
        use bevy::ui::{ui_layout_system, ui_surface::UiSurface, LayoutConfig};

        let mut world = World::new();
        world.init_resource::<UiSurface>();
        world.init_resource::<bevy::text::FontCx>();
        world.init_resource::<bevy::text::RemSize>();
        let (_, _, scale) = sheet_layout(1360., 860., 240., 297., 210.);
        let thickness = 0.25 * scale;
        let mut extensions = Vec::new();
        for use_rounding in [true, false] {
            let paper = world
                .spawn((
                    rect(356., 132., 297. * scale, 210. * scale),
                    LayoutConfig { use_rounding },
                ))
                .id();
            let length = (76.7 - 68.5) * scale;
            let midpoint = Vec2::new(60. * scale, 72.6 * scale);
            let extension = world
                .spawn((
                    rect(
                        midpoint.x - length * 0.5,
                        midpoint.y - thickness * 0.5,
                        length,
                        thickness,
                    ),
                    UiTransform::from_rotation(Rot2::radians(std::f32::consts::FRAC_PI_2)),
                    ChildOf(paper),
                ))
                .id();
            extensions.push(extension);
        }
        world.run_system_cached(ui_layout_system).unwrap();
        let rounded = world.get::<ComputedNode>(extensions[0]).unwrap();
        assert_eq!(
            rounded.size().y,
            0.,
            "fixture must reproduce the lost stroke"
        );
        let unrounded = world.get::<ComputedNode>(extensions[1]).unwrap();
        assert!((unrounded.size().y - thickness).abs() < 0.0001);
        assert!(unrounded.size().x > 24.);
    }

    #[test]
    fn paper_fit_reserves_browser_ribbon_and_history_controls() {
        for (width, height, side) in [
            (800., 600., 240.),
            (1360., 860., 240.),
            (1920., 1080., 280.),
        ] {
            for (sheet_w, sheet_h) in [(297., 210.), (210., 297.), (1189., 841.)] {
                let (x, y, scale) = sheet_layout(width, height, side, sheet_w, sheet_h);
                assert!(scale > 0.);
                assert!(x >= side + 15.9);
                assert!(x + sheet_w * scale <= width - 15.9);
                assert!(y >= 132.);
                assert!(y + sheet_h * scale <= height - 99.9);
            }
        }
    }

    #[test]
    fn paper_point_centers_the_projection_on_the_view() {
        let view = DrawingViewDto {
            scope: Default::default(),
            occurrence_ids: vec![],
            id: 1,
            name: "Front".into(),
            kind: DrawingViewKind::Front,
            direction: [0., -1., 0.],
            up: [0., 0., 1.],
            position: [100., 80.],
            scale: 2.,
            body_ids: vec![],
            show_hidden_lines: false,
            show_tangent_edges: false,
            parent_view_id: None,
            alignment: Default::default(),
            derivation: None,
        };
        let projection = DrawingProjectionDto {
            topology_signatures: Default::default(),
            visible: vec![],
            hidden: vec![],
            anchors: vec![],
            circles: vec![],
            section: vec![],
            bounds: [0., 0., 10., 4.],
        };
        let center = paper_point(&view, [5., 2.], &projection);
        assert_eq!(center, [100., 80.]);
        let corner = paper_point(&view, [10., 4.], &projection);
        assert_eq!(corner, [110., 76.]);
        let label = view_name_label(&view, &projection, 2.5, None);
        assert_eq!(label.text, "Front · 2:1");
        assert_eq!(label.x, 100.);
        let top = f64::from(label.y) - f64::from(label.height_mm) * 0.5;
        assert!(top + 1e-6 >= 84. + 6. + 1.2 + 0.125 + 1.);
        let mut reduced = view.clone();
        reduced.scale = 0.5;
        let label = view_name_label(&reduced, &projection, 2.5, None);
        assert_eq!(label.text, "Front · 1:2");
        let top = f64::from(label.y) - f64::from(label.height_mm) * 0.5;
        assert!(top + 1e-6 >= 81. + 6. + 1.2 + 0.125 + 1.);
    }

    #[test]
    fn horizontal_dimension_offset_is_paper_millimetres() {
        let (value, _, _, c, d) = dimension_span(
            limo_cad_sketch::DrawingLinearDimensionMode::Horizontal,
            [10., 20.],
            [40., 22.],
            8.,
            1.,
        )
        .unwrap();
        assert_eq!(value, 30.);
        assert_eq!(c, [10., 28.]);
        assert_eq!(d, [40., 28.]);
    }

    #[test]
    fn scaled_dimensions_keep_model_measurement_and_paper_offset() {
        use limo_cad_sketch::DrawingLinearDimensionMode::{Aligned, Horizontal, Vertical};
        for (mode, second, expected) in [
            (Horizontal, [40., 20.], 30.),
            (Vertical, [10., 60.], 40.),
            (Aligned, [40., 60.], 50.),
        ] {
            for scale in [0.5, 1., 2.] {
                let first = [10. * scale, 20. * scale];
                let second = [second[0] * scale, second[1] * scale];
                let (value, _, _, c, _) = dimension_span(mode, first, second, 8., scale).unwrap();
                assert_eq!(value, expected);
                assert!(((c[0] - first[0]).hypot(c[1] - first[1]) - 8.).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn drawing_annotation_repaints_without_a_solid_revision() {
        use crate::session_bridge::native_interface::tests::Fixture;
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        let mutate = |operation, arguments| {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, operation, &arguments, || Ok(()))
                .unwrap();
        };
        for (operation, arguments) in [
            (
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            ),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
            ),
            (
                "drawing_create_sheet",
                json!({"name":"Drawing test","format":"a4","orientation":"landscape"}),
            ),
            (
                "drawing_add_view",
                json!({"sheet_id":1,"view":{"name":"Front 2:1","kind":"front","direction":[0.,-1.,0.],"up":[0.,0.,1.],"position":[100.,100.],"scale":2.}}),
            ),
        ] {
            mutate(operation, arguments);
        }
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let mut app = native_viewport::interface_scene_fixture();
        let world = app.world_mut();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<ViewportUiAssets>();
        let camera = world.spawn(InterfaceCamera).id();
        let mut state = Workbench::default();
        state.refresh_owner(&owner);
        state.widgets.begin();
        paint(
            world,
            camera,
            &services,
            &mut state,
            (1200., 800., 240.),
            &HashMap::new(),
        )
        .unwrap();
        state.widgets.finish(world);
        assert_eq!(state.paper_labels.len(), 1);
        assert_eq!(state.paper_labels[0].ink, Ink::ViewName);
        let view_label = state.paper_labels[0].text.clone();
        assert!(!state
            .paper_labels
            .iter()
            .any(|label| label.ink == Ink::Drawing));
        let paper = state.widgets.entity("drawing-paper").unwrap();
        assert!(
            !world
                .get::<bevy::ui::LayoutConfig>(paper)
                .unwrap()
                .use_rounding
        );
        assert_eq!(
            world.get::<BackgroundColor>(paper),
            Some(&BackgroundColor(Color::WHITE))
        );
        assert_eq!(world.get::<Node>(paper).unwrap().overflow, Overflow::clip());
        assert!(world
            .get::<interface_shell::InterfaceOccluder>(
                state.widgets.entity("drawing-backdrop").unwrap()
            )
            .is_some());
        let geometry_revision = services.engine.geometry_revision();
        let drawing = services.engine.drawing_snapshot();
        let projection = services
            .engine
            .project_sheet_view(&drawing.sheets[0].views[0], &drawing.sheets[0].views)
            .unwrap();
        let (first, second) = projection
            .anchors
            .iter()
            .find_map(|first| {
                projection
                    .anchors
                    .iter()
                    .find(|second| {
                        first.edge_id == second.edge_id
                            && first.endpoint != second.endpoint
                            && ((first.point[0] - second.point[0]).abs() - 40.).abs() < 1e-8
                    })
                    .map(|second| (first, second))
            })
            .unwrap();
        let anchor = |row: &limo_cad_occt::DrawingProjectionAnchorDto| {
            json!({
                "body_id":row.body_id,"edge_id":row.edge_id,"edge_key":row.edge_key,
                "endpoint":row.endpoint,"fallback_point":row.model_point,
            })
        };
        let export = || {
            crate::session_bridge::parse_engine_envelope(
                fixture.engine.engine_call("project_export_model", ""),
            )
            .unwrap()
        };
        let before_dimension = export();
        let solid = serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap();
        let inbox = |seq, operation, arguments| {
            let session = fixture
                .bridge
                .session_id_for_window("main")
                .unwrap()
                .unwrap();
            let revision = fixture
                .bridge
                .engine_revision_for_window("main")
                .unwrap()
                .unwrap();
            let directory = crate::session_bridge::inbox_dir(&session);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join(format!("{seq}.json")),
                json!({"name":operation,"arguments":arguments,"base_generation":revision,
                    "session_id":session,"window_id":"main"})
                .to_string(),
            )
            .unwrap();
            crate::session_bridge::apply_or_reject_one_inbox_op(
                &fixture.bridge,
                "main",
                &fixture.engine,
                None,
                Some((&owner.document_id, &session)),
            )
            .unwrap()
        };
        assert_eq!(
            inbox(
                1,
                "drawing_add_linear_dimension",
                json!({
                    "sheet_id":1,"view_id":1,"first":anchor(first),"second":anchor(second),
                    "mode":"horizontal","offset":12.,"precision":2,
                }),
            )["applied"],
            true
        );
        let with_dimension = export();
        assert_eq!(services.engine.geometry_revision(), geometry_revision);
        state.widgets.begin();
        paint(
            world,
            camera,
            &services,
            &mut state,
            (1200., 800., 240.),
            &HashMap::new(),
        )
        .unwrap();
        state.widgets.finish(world);
        let dimensions: Vec<_> = state
            .paper_labels
            .iter()
            .filter(|label| label.ink == Ink::Drawing)
            .collect();
        assert_eq!(dimensions.len(), 1);
        assert_eq!(dimensions[0].text, "40.00 mm");
        assert_eq!(
            state
                .paper_labels
                .iter()
                .filter(|label| label.ink == Ink::ViewName)
                .map(|label| label.text.as_str())
                .collect::<Vec<_>>(),
            [view_label.as_str()]
        );
        let arrows: Vec<_> = state
            .paper
            .iter()
            .enumerate()
            .filter(|(_, segment)| segment.arrow)
            .map(|(index, _)| {
                state
                    .widgets
                    .entity(&format!("drawing-edge-{index}"))
                    .unwrap()
            })
            .collect();
        assert_eq!(arrows.len(), 2);
        assert!(arrows
            .iter()
            .all(|entity| world.get::<ImageNode>(*entity).is_some()));
        assert_eq!(
            world.get::<ImageNode>(arrows[0]).unwrap().image,
            world.get::<ImageNode>(arrows[1]).unwrap().image,
            "Both arrowheads reuse one rasterized triangle"
        );
        let label = state.widgets.entity("drawing-dimension-0").unwrap();
        let label_box = world.get::<ChildOf>(label).unwrap().parent();
        assert_eq!(world.get::<ChildOf>(label_box).unwrap().parent(), paper);
        assert_eq!(
            world.get::<TextLayout>(label).unwrap().linebreak,
            bevy::text::LineBreak::NoWrap
        );
        assert!(world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0 == "40.00 mm"));

        let rejected = inbox(
            2,
            "drawing_add_note",
            json!({"sheet_id":999,"text":"Rejected","position":[20.,20.]}),
        );
        assert_eq!(rejected["applied"], false);
        assert_eq!(rejected["dead_lettered"], true);
        assert_eq!(export(), with_dimension);
        let revision = fixture
            .bridge
            .engine_revision_for_window("main")
            .unwrap()
            .unwrap();
        fixture
            .bridge
            .publishers
            .lock()
            .unwrap()
            .get_mut("main")
            .unwrap()
            .active_mut()
            .engine_revision = u64::MAX;
        let exhausted = inbox(
            3,
            "drawing_add_note",
            json!({"sheet_id":1,"text":"Must not dispatch","position":[20.,20.]}),
        );
        assert_eq!(exhausted["applied"], false);
        assert_eq!(exhausted["dead_lettered"], true);
        assert!(exhausted["error"]
            .as_str()
            .unwrap()
            .contains("revision exhausted"));
        assert_eq!(export(), with_dimension);
        fixture
            .bridge
            .publishers
            .lock()
            .unwrap()
            .get_mut("main")
            .unwrap()
            .active_mut()
            .engine_revision = revision;

        fixture
            .bridge
            .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(export(), before_dimension);
        assert_eq!(
            serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap(),
            solid
        );
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(export(), with_dimension);
        assert_eq!(
            serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap(),
            solid
        );
    }

    #[test]
    fn drawing_edits_and_sheet_selection_undo_without_deleting_the_solid() {
        use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let mutate = |operation, arguments| {
            fixture
                .bridge
                .apply_native_mutation(
                    &fixture.engine,
                    &fixture.owner(),
                    operation,
                    &arguments,
                    || Ok(()),
                )
                .unwrap();
        };
        let export = || {
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap()
        };
        for (operation, arguments) in [
            (
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            ),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
            ),
        ] {
            mutate(operation, arguments);
        }
        let solid = serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap();
        let mut snapshots = vec![export()];
        for (operation, arguments) in [
            (
                "drawing_create_sheet",
                json!({"name":"Original sheet","format":"a4","orientation":"landscape"}),
            ),
            (
                "drawing_add_view",
                json!({"sheet_id":1,"view":{"name":"Front","kind":"front","direction":[0.,-1.,0.],"up":[0.,0.,1.],"position":[100.,100.],"scale":1.}}),
            ),
            (
                "drawing_add_note",
                json!({"sheet_id":1,"text":"Preserve the solid","position":[20.,20.]}),
            ),
            (
                "drawing_create_sheet",
                json!({"name":"Scratch sheet","format":"a4","orientation":"portrait"}),
            ),
            ("drawing_select_sheet", json!({"sheet_id":1})),
            ("drawing_delete_sheet", json!({"sheet_id":2})),
        ] {
            mutate(operation, arguments);
            snapshots.push(export());
        }
        for expected in snapshots[..snapshots.len() - 1].iter().rev() {
            fixture
                .bridge
                .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
                .unwrap();
            assert_eq!(export(), *expected);
            assert_eq!(
                serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap(),
                solid
            );
        }
        for expected in &snapshots[1..] {
            fixture
                .bridge
                .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
                .unwrap();
            assert_eq!(export(), *expected);
            assert_eq!(
                serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap(),
                solid
            );
        }
    }
}
