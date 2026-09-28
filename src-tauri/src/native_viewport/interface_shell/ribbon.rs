//! Original ribbon geometry and typography on retained native controls.
//! Both renderers consume the same SVG sources. Rust rasterizes each vector
//! once, then Bevy draws a cached texture; there is no webview here.
use super::*;
use bevy::{
    asset::RenderAssetUsages,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    text::{LetterSpacing, LineBreak, LineHeight},
};
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct RibbonButton {
    pub finish: bool,
    display_label: String,
}
impl RibbonButton {
    pub(super) fn fill(
        &self,
        theme: ViewportUiTheme,
        active: bool,
        hover: bool,
        disabled: bool,
    ) -> Color {
        if self.finish {
            return if hover && !disabled {
                Color::srgb(88. / 255. * 1.1, 166. / 255. * 1.1, 92. / 255. * 1.1)
            } else {
                Color::srgb_u8(88, 166, 92)
            };
        }
        if disabled {
            Color::NONE
        } else if active {
            // CSS composites opacity in sRGB; leaving alpha for Bevy's linear
            // framebuffer makes selected cells visibly brighter than the source.
            css_mix(theme.accent, theme.header, if hover { 0.30 } else { 0.25 })
        } else if hover {
            theme.edge.with_alpha(1.)
        } else {
            Color::NONE
        }
    }
    pub(super) fn ink(&self, theme: ViewportUiTheme, disabled: bool) -> Color {
        if self.finish {
            Color::WHITE
        } else if disabled {
            css_mix(theme.mute, theme.header, 0.4)
        } else {
            theme.mute
        }
    }
    pub(super) fn label(&self) -> &str {
        &self.display_label
    }
}

pub(crate) fn css_mix(foreground: Color, background: Color, opacity: f32) -> Color {
    let fg = foreground.to_srgba();
    let bg = background.to_srgba();
    Color::srgb(
        fg.red * opacity + bg.red * (1. - opacity),
        fg.green * opacity + bg.green * (1. - opacity),
        fg.blue * opacity + bg.blue * (1. - opacity),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Icon {
    Orbit,
    Pan,
    Zoom,
    ZoomWindow,
    Fit,
    Monitor,
    Grid,
    Gamepad,
    Undo,
    Redo,
    History,
    Measure,
    Book,
    FolderOpen,
    Import,
    Export,

    Extrude,
    Revolve,
    Sweep,
    Loft,
    Rib,
    Sketch,
    Line,
    MidpointLine,
    Rectangle,
    Circle,
    Arc,
    Slot,
    Point,
    Spline,
    Finish,
    Cancel,
    Chevron,
    Box,
    Boxes,
    Joint,
    Save,
    Activity,
    Play,
    Pause,
    ShieldAlert,
    TimerReset,
    Anchor,
    Copy,
    FolderTree,
    Bookmark,
    Crosshair,
    Square,
    CircleDot,
    PenLine,
    Layers,
    Settings,
    ChevronRight,
    ChevronDown,
    Eye,
    EyeOff,
    Pencil,
    Globe,
    ArrowLeft,
    ArrowRight,
    ArrowLeftToLine,
    ArrowRightToLine,
    Trim,
    Extend,
    Break,
    Dimension,
    Select,
    Relation(&'static str),
    Offset,
    Fillet,
    Chamfer,
    Shell,
    ExternalThread,
    Hole,
    Combine,
    OffsetPlane,
    Midplane,
    AnglePlane,
    SplitBody,
    MoveCopy,
    Mirror,
    RectangularPattern,
    CircularPattern,
}
impl Icon {
    fn svg(self) -> &'static str {
        macro_rules! source {
            ($name:literal) => {
                include_str!(concat!(
                    "../../../../src/assets/ribbon-icons/",
                    $name,
                    ".svg"
                ))
            };
        }
        match self {
            Self::Orbit => source!("move-3d"),
            Self::Pan => source!("hand"),
            Self::Zoom => source!("zoom-in"),
            Self::ZoomWindow => source!("square-dashed"),
            Self::Fit => source!("maximize"),
            Self::Monitor => source!("monitor"),
            Self::Grid => source!("grid-3x3"),
            Self::Gamepad => source!("gamepad-2"),
            Self::Undo => source!("undo-2"),
            Self::Redo => source!("redo-2"),
            Self::History => source!("history"),
            Self::Measure => source!("ruler"),
            Self::Book => source!("book-open"),
            Self::FolderOpen => source!("folder-open"),
            Self::Import => source!("file-up"),
            Self::Export => source!("file-down"),

            Self::Extrude => source!("extrude"),
            Self::Revolve => source!("revolve"),
            Self::Sweep => source!("sweep"),
            Self::Loft => source!("loft"),
            Self::Rib => source!("rib"),
            Self::Sketch => source!("sketch"),
            Self::Line => source!("line"),
            Self::MidpointLine => source!("midpointLine"),
            Self::Rectangle => source!("rect"),
            Self::Circle => source!("circle"),
            Self::Arc => source!("arc"),
            Self::Slot => source!("slot"),
            Self::Point => source!("point"),
            Self::Spline => source!("spline"),
            Self::Finish => source!("finish"),
            Self::Cancel => source!("cancel"),
            Self::Chevron => source!("chevron"),
            Self::Box => source!("box"),
            Self::Boxes => source!("boxes"),
            Self::Joint => source!("joint"),
            Self::Save => source!("save"),
            Self::Activity => source!("activity"),
            Self::Play => source!("play"),
            Self::Pause => source!("pause"),
            Self::ShieldAlert => source!("shield-alert"),
            Self::TimerReset => source!("timer-reset"),
            Self::Anchor => source!("anchor"),
            Self::Copy => source!("copy"),
            Self::FolderTree => source!("folder-tree"),
            Self::Bookmark => source!("bookmark"),
            Self::Crosshair => source!("crosshair"),
            Self::Square => source!("square"),
            Self::CircleDot => source!("circle-dot"),
            Self::PenLine => source!("pen-line"),
            Self::Layers => source!("layers-3"),
            Self::Settings => source!("sliders-horizontal"),
            Self::ChevronRight => source!("chevron-right"),
            Self::ChevronDown => source!("chevron-down"),
            Self::Eye => source!("eye"),
            Self::EyeOff => source!("eye-off"),
            Self::Pencil => source!("pencil"),
            Self::Globe => source!("globe"),
            Self::ArrowLeft => source!("arrow-left"),
            Self::ArrowRight => source!("arrow-right"),
            Self::ArrowLeftToLine => source!("arrow-left-to-line"),
            Self::ArrowRightToLine => source!("arrow-right-to-line"),
            Self::Trim => source!("trim"),
            Self::Extend => source!("extend"),
            Self::Break => source!("break"),
            Self::Dimension => source!("dim"),
            Self::Select => source!("select"),
            Self::Offset => source!("offset"),
            Self::Fillet => source!("fillet"),
            Self::Chamfer => source!("chamfer"),
            Self::Shell => source!("shell"),
            Self::ExternalThread => source!("externalThread"),
            Self::Hole => source!("hole"),
            Self::Combine => source!("combine"),
            Self::OffsetPlane => source!("plane"),
            Self::Midplane => source!("midplane"),
            Self::AnglePlane => source!("planeAngle"),
            Self::SplitBody => source!("splitBody"),
            Self::MoveCopy => source!("moveCopy"),
            Self::Mirror => source!("mirror"),
            Self::RectangularPattern => source!("rectPattern"),
            Self::CircularPattern => source!("circPattern"),
            Self::Relation(name) => match name {
                "hv" => source!("hv"),
                "coincident" => source!("coincident"),
                "tangent" => source!("tangent"),
                "equal" => source!("equal"),
                "parallel" => source!("parallel"),
                "perpendicular" => source!("perpendicular"),
                "fix" => source!("fix"),
                "midpointC" => source!("midpointC"),
                "concentric" => source!("concentric"),
                "collinear" => source!("collinear"),
                "symmetry" => source!("symmetry"),
                _ => unreachable!("Known relation glyph"),
            },
        }
    }
}
#[derive(Resource, Default)]
pub(super) struct GlyphCache(HashMap<(Icon, u32), Handle<Image>>);

/// Rasterize at the actual physical widget size. A large texture minified
/// without mipmaps aliases fine strokes instead of improving their quality.
/// Color belongs to the widget, not to a separate raster for every state.
fn rasterize(icon: Icon, pixels: u32) -> Image {
    let source = icon.svg().replace("currentColor", "white");
    let tree = resvg::usvg::Tree::from_str(&source, &resvg::usvg::Options::default())
        .expect("validated built-in ribbon SVG");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(pixels as f32 / 24., pixels as f32 / 24.),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity((pixels * pixels * 4) as usize);
    for pixel in pixmap.pixels() {
        let pixel = pixel.demultiply();
        rgba.extend_from_slice(&[pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]);
    }
    Image::new(
        Extent3d {
            width: pixels,
            height: pixels,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
fn image(world: &mut World, icon: Icon, pixels: u32) -> Handle<Image> {
    world.init_resource::<Assets<Image>>();
    world.init_resource::<GlyphCache>();
    if let Some(image) = world.resource::<GlyphCache>().0.get(&(icon, pixels)) {
        return image.clone();
    }
    let image = world
        .resource_mut::<Assets<Image>>()
        .add(rasterize(icon, pixels));
    world
        .resource_mut::<GlyphCache>()
        .0
        .insert((icon, pixels), image.clone());
    image
}
#[derive(Component)]
pub(super) struct RibbonGlyph {
    owner: Entity,
    icon: Icon,
    ink: GlyphInk,
}
#[derive(Clone, Copy, PartialEq)]
enum GlyphInk {
    Text,
    Muted,
    Fixed(Color),
}
impl GlyphInk {
    fn resolve(self, theme: ViewportUiTheme) -> Color {
        match self {
            Self::Text => theme.ink,
            Self::Muted => theme.mute,
            Self::Fixed(ink) => ink,
        }
    }
}
pub(super) fn update_glyphs(
    controls: Query<(&InterfaceControl, &InterfaceButtonStyle)>,
    mut glyphs: Query<(&RibbonGlyph, &mut ImageNode, Option<&ComputedNode>)>,
    cache: Option<ResMut<GlyphCache>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(mut cache) = cache else {
        return;
    };
    for (glyph, mut image, node) in &mut glyphs {
        if let Some(node) = node.filter(|node| node.size().x > 0.) {
            let pixels = (node.size().x.round() as u32).clamp(1, 512);
            let texture = cache
                .0
                .entry((glyph.icon, pixels))
                .or_insert_with(|| images.add(rasterize(glyph.icon, pixels)));
            if image.image != *texture {
                image.image = texture.clone();
            }
        }
        if let Ok((control, style)) = controls.get(glyph.owner) {
            let color = if control.disabled {
                css_mix(style.0.mute, style.0.header, 0.4)
            } else {
                glyph.ink.resolve(style.0)
            };
            if image.color != color {
                image.color = color;
            }
        }
    }
}
pub(crate) fn node(x: f32, y: f32, width: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(width),
        height: px(52.),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        border_radius: BorderRadius::all(px(4.)),
        ..default()
    }
}
pub(crate) fn finish_node(right: f32, y: f32, compact: bool) -> Node {
    Node {
        position_type: PositionType::Absolute,
        right: px(right),
        top: px(y),
        height: px(32.),
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(6.),
        padding: UiRect::horizontal(px(if compact { 8. } else { 12. })),
        border_radius: BorderRadius::all(px(4.)),
        ..default()
    }
}
fn glyph(
    world: &mut World,
    owner: Entity,
    icon: Icon,
    x: f32,
    y: f32,
    size: f32,
    ink: GlyphInk,
) -> Entity {
    let image = image(world, icon, size.round() as u32);
    let finish = world
        .get::<RibbonButton>(owner)
        .is_some_and(|button| button.finish);
    let theme = world.get::<InterfaceButtonStyle>(owner).unwrap().0;
    let child = world
        .spawn((
            Node {
                position_type: if finish {
                    PositionType::Relative
                } else {
                    PositionType::Absolute
                },
                left: if finish { Val::Auto } else { px(x) },
                top: if finish { Val::Auto } else { px(y) },
                width: px(size),
                height: px(size),
                flex_shrink: 0.,
                ..default()
            },
            ImageNode {
                image,
                color: ink.resolve(theme),
                ..default()
            },
            RibbonGlyph {
                owner,
                icon,
                ink,
            },
        ))
        .id();
    world.entity_mut(owner).add_child(child);
    child
}
/// Use the same cached SVG pipeline for compact chrome and tree controls.
pub(crate) fn compact_glyph(
    world: &mut World,
    owner: Entity,
    icon: Icon,
    x: f32,
    size: f32,
) -> Entity {
    glyph(world, owner, icon, x, (24. - size) / 2., size, GlyphInk::Muted)
}

pub(crate) fn center_glyph(world: &mut World, owner: Entity) {
    let entities: Vec<Entity> = world.query::<(Entity, &RibbonGlyph)>().iter(world).filter(|(_, g)| g.owner == owner).map(|(e, _)| e).collect();
    for entity in entities {
        if let Some(mut node) = world.get_mut::<Node>(entity) {
            let size = if let Val::Px(size) = node.width { size } else { 13. };
            node.left = percent(50.); node.top = percent(50.);
            node.margin = UiRect { left: px(-size / 2.), top: px(-size / 2.), ..default() };
        }
    }
}

pub(crate) fn replace_compact_glyph(world: &mut World, owner: Entity, icon: Icon) {
    let mut glyphs = world.query::<&mut RibbonGlyph>();
    for mut glyph in glyphs.iter_mut(world).filter(|glyph| glyph.owner == owner) {
        if glyph.icon != icon {
            glyph.icon = icon;
        }
    }
}

pub(crate) fn decoration(world: &mut World, camera: Entity, icon: Icon, ink: Color) -> Entity {
    let image = image(world, icon, 13);
    let entity = world.spawn_empty().id();
    world.entity_mut(entity).insert((
        UiTargetCamera(camera),
        ZIndex(30),
        ImageNode {
            image,
            color: ink,
            ..default()
        },
        RibbonGlyph {
            owner: entity,
            icon,
            ink: GlyphInk::Fixed(ink),
        },
    ));
    entity
}
pub(crate) fn refresh_decoration(world: &mut World, entity: Entity, icon: Icon, ink: Color) {
    if let Some(mut glyph) = world.get_mut::<RibbonGlyph>(entity) {
        if glyph.icon != icon { glyph.icon = icon; }
        if glyph.ink != GlyphInk::Fixed(ink) { glyph.ink = GlyphInk::Fixed(ink); }
    }
    if let Some(mut image) = world.get_mut::<ImageNode>(entity) {
        if image.color != ink { image.color = ink; }
    }
}
pub(crate) fn caption(world: &mut World, entity: Entity, value: &str) {
    let Some(mut button) = world.get_mut::<RibbonButton>(entity) else { return; };
    if button.display_label != value {
        button.display_label = value.into();
        let finish = button.finish;
        drop(button);
        let label = world.get::<InterfaceLabel>(entity).unwrap().0;
        world.entity_mut(label).insert(caption_bounds(finish, value));
    }
}

fn caption_bounds(finish: bool, value: &str) -> Node {
    let lines = if value.contains('\n') || (!finish && value.chars().count() > 11) { 2. } else { 1. };
    Node {
        position_type: if finish { PositionType::Relative } else { PositionType::Absolute },
        top: if finish { Val::Auto } else { px(40. - lines * 4.) },
        left: if finish { Val::Auto } else { px(0.) },
        width: if finish { Val::Auto } else { percent(100.) },
        height: if finish { Val::Auto } else { px(16.) },
        min_width: px(0.),
        flex_shrink: 0.,
        overflow: Overflow::clip(),
        ..default()
    }
}

/// A translated long word must wrap inside its existing group cell rather
/// than forcing the neighboring caption or chevron out of the ribbon.
pub(crate) fn group_caption(world: &mut World, entity: Entity, available_width: f32) {
    let Some(label) = world.get::<InterfaceLabel>(entity).map(|label| label.0) else { return; };
    // A collapsed one-command group has 36px beside its chevron. Long
    // translated titles use the same 8px size as the tool captions there;
    // ordinary group cells keep their existing 10px tracked typography.
    let narrow = available_width < 48.;
    super::caption_size(world, entity, if narrow { 8. } else { 10. });
    let tracking = LetterSpacing::Px(if narrow { 0. } else { 0.5 });
    if world.get::<LetterSpacing>(label) != Some(&tracking) { world.entity_mut(label).insert(tracking); }
    // WordOrCharacter has a one-glyph min-content width. Give the label the
    // actual remaining cell width so flex measurement cannot collapse every
    // group title into a narrow column beside its menu chevron.
    let node = Node { width: px(available_width.max(0.)), min_width: px(0.),
        flex_shrink: 0., max_height: px(20.),
        margin: UiRect::ZERO, overflow: Overflow::clip(), ..default() };
    let layout = TextLayout::new(Justify::Center, LineBreak::WordOrCharacter);
    if world.get::<Node>(label) != Some(&node) { world.entity_mut(label).insert(node); }
    if world.get::<TextLayout>(label).is_none_or(|current| current.justify != layout.justify || current.linebreak != layout.linebreak) {
        world.entity_mut(label).insert(layout);
    }
    if world.get::<LineHeight>(label) != Some(&LineHeight::Px(10.)) { world.entity_mut(label).insert(LineHeight::Px(10.)); }
}

pub(crate) fn decorate(world: &mut World, entity: Entity, icon: Icon) {
    if world.get::<RibbonButton>(entity).is_some() {
        return;
    }
    let label = world.get::<InterfaceLabel>(entity).unwrap().0;
    let theme = world.get::<InterfaceButtonStyle>(entity).unwrap().0;
    let assets = world.resource::<ViewportUiAssets>().clone();
    let semantic = &world.get::<InterfaceControl>(entity).unwrap().label;
    let display_label = match semantic.as_str() {
        "Three-point arc" => "Arc",
        "Fit-point spline" => "Spline",
        "Center-to-center slot" => "Slot",
        "Create Sketch" => "Create\nSketch",
        "Rectangular Pattern" => "Rectangular\nPattern",
        "Circular Pattern" => "Circular\nPattern",
        "External Thread" => "External\nThread",
        "Offset Plane" => "Offset\nPlane",
        "Plane at Angle" => "Angled plane",
        "Finish sketch" => "FINISH SKETCH",
        "Finish spline" => "FINISH SPLINE",
        other => other,
    }
    .to_owned();
    let finish = matches!(icon, Icon::Finish);
    world.entity_mut(label).insert((
        Text::new(&display_label),
        theme.text(
            &assets,
            if finish { 11. } else { 8. },
            if finish {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            },
        ),
        TextLayout::new(Justify::Center, LineBreak::WordOrCharacter),
        FontHinting::Enabled,
        LineHeight::Px(if finish { 16.5 } else { 8. }),
        LetterSpacing::Px(if finish { 0.275 } else { 0. }),
        caption_bounds(finish, &display_label),
    ));
    world.entity_mut(entity).insert(RibbonButton {
        finish,
        display_label,
    });
    let primary_glyph = glyph(
        world,
        entity,
        icon,
        if finish { 8. } else { 0. },
        if finish { 9. } else { 5. },
        if finish { 14. } else { 22. },
        if finish {
            GlyphInk::Fixed(Color::WHITE)
        } else if matches!(icon, Icon::Relation(_)) {
            GlyphInk::Fixed(Color::srgb_u8(224, 120, 120))
        } else {
            GlyphInk::Text
        },
    );
    if !finish {
        if let Some(mut node) = world.get_mut::<Node>(primary_glyph) {
            node.left = percent(50.);
            node.margin.left = px(-11.);
        }
    }
    if finish {
        world
            .entity_mut(entity)
            .insert_children(0, &[primary_glyph]);
        glyph(
            world,
            entity,
            Icon::Chevron,
            121.,
            10.5,
            11.,
            GlyphInk::Fixed(Color::WHITE.with_alpha(0.7)),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_titles_shape_inside_their_computed_cells_with_room_for_chevrons() {
        use bevy::{asset::AssetPlugin, text::{ComputedTextBlock, TextLayoutInfo, TextPlugin}};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TextPlugin))
            .init_resource::<Assets<Image>>()
            .init_resource::<bevy::ui::ui_surface::UiSurface>()
            .add_systems(Startup, crate::native_viewport::ui::load_system_font);
        app.update();
        app.update();
        let assets = app.world().resource::<ViewportUiAssets>().clone();
        let theme = ViewportUiTheme::from_palette(&crate::native_viewport::ViewportPalette::default());
        let camera = app.world_mut().spawn_empty().id();
        // These are actual single- and multi-button group cell widths. The
        // narrow translated title must wrap, while ordinary titles remain
        // complete. Shape text and run Bevy's flex layout, not just Node checks.
        for (title, width, has_menu) in [
            ("PROFILE", 98., true), ("BUILD", 298., true),
            ("REFERENCE", 98., true), ("CHECK", 48., false),
            ("WIEDERHOLEN", 48., true), ("BAUGRUPPE", 48., true),
            ("REFERENCIA", 48., true),
            ("AUSWÄHLEN", 48., false),
        ] {
            let button = spawn_button(&mut app.world_mut().commands(), camera,
                Node { width: px(width), height: px(20.), justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center, ..default() },
                InterfaceControl::button("test", title), theme, &assets);
            app.world_mut().flush();
            super::super::center_caption(app.world_mut(), button);
            super::super::caption_size(app.world_mut(), button, 10.);
            super::super::caption_tracking(app.world_mut(), button, 0.5);
            let label = app.world().get::<InterfaceLabel>(button).unwrap().0;
            let available = width - if has_menu { 12. } else { 0. };
            group_caption(app.world_mut(), button, available);
            let chevron = has_menu.then(|| {
                let entity = app.world_mut().spawn(Node { width: px(10.), height: px(10.),
                    margin: UiRect::left(px(2.)), flex_shrink: 0., ..default() }).id();
                app.world_mut().entity_mut(button).add_child(entity);
                entity
            });
            app.update();
            app.world_mut().run_system_cached(bevy::ui::widget::measure_text_system).unwrap();
            app.world_mut().run_system_cached(bevy::ui::ui_layout_system).unwrap();
            app.world_mut().run_system_cached(bevy::ui::widget::text_system).unwrap();
            let bounds = app.world().get::<ComputedNode>(label).unwrap().size();
            assert_eq!(bounds.x, available, "{title} collapsed to min-content width");
            assert!(bounds.y > 0. && bounds.y <= 20., "{title}: {bounds:?}");
            let text = app.world().get::<ComputedTextBlock>(label).unwrap();
            assert!((1..=2).contains(&text.buffer().lines().count()), "{title} must fit in two lines");
            let layout = app.world().get::<TextLayoutInfo>(label).unwrap();
            assert_eq!(layout.glyphs.len(), title.chars().count(), "{title} lost shaped glyphs");
            assert!(layout.size.x <= available + 0.1 && layout.size.y <= 20.1,
                "{title} exceeds its cell: {:?}", layout.size);
            if let Some(chevron) = chevron {
                let label_position = app.world().get::<UiGlobalTransform>(label).unwrap().translation;
                let chevron_position = app.world().get::<UiGlobalTransform>(chevron).unwrap().translation;
                assert!(label_position.x + bounds.x / 2. <= chevron_position.x - 5. + 0.1,
                    "{title} overlaps its chevron");
            }
        }
    }

    #[test]
    fn retained_glyphs_follow_theme_without_replacing_fixed_colors_or_bindings() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .insert_resource(ViewportUiAssets::default())
            .add_systems(Update, update_glyphs);
        let mut light = ViewportUiTheme::from_palette(&crate::native_viewport::ViewportPalette::default());
        light.ink = Color::srgb(0.1, 0.1, 0.1);
        light.mute = Color::srgb(0.4, 0.4, 0.4);
        light.header = Color::srgb(0.9, 0.9, 0.9);
        let mut dark = light;
        dark.ink = Color::srgb(0.9, 0.9, 0.9);
        dark.mute = Color::srgb(0.7, 0.7, 0.7);
        dark.header = Color::srgb(0.1, 0.1, 0.1);
        let camera = app.world_mut().spawn_empty().id();
        let mut buttons = Vec::new();
        for (caption, icon) in [("Extrude", Some(Icon::Extrude)), ("Undo", None),
            ("Finish sketch", Some(Icon::Finish)), ("Coincident", Some(Icon::Relation("coincident")))] {
            let button = spawn_button(&mut app.world_mut().commands(), camera, node(0.,0.,48.),
                InterfaceControl::button("test", caption), light, &ViewportUiAssets::default());
            app.world_mut().flush();
            if let Some(icon) = icon { decorate(app.world_mut(),button,icon); }
            else { compact_glyph(app.world_mut(),button,Icon::Undo,0.,13.); }
            buttons.push((button,app.world().get::<InterfaceControl>(button).unwrap().clone()));
        }
        let explicit = Color::srgb(0.8, 0.2, 0.1);
        let decoration = decoration(app.world_mut(),camera,Icon::Eye,explicit);
        app.update();
        let original: Vec<_> = app.world_mut().query::<(Entity,&ImageNode)>().iter(app.world())
            .map(|(entity,image)|(entity,image.image.clone())).collect();
        for theme in [dark, light, dark] {
            super::super::refresh_theme(app.world_mut(),theme);
            app.update();
            for (button,control) in &buttons {
                assert_eq!(app.world().get::<InterfaceControl>(*button).unwrap(),control);
            }
            for (entity,image) in &original {
                assert_eq!(&app.world().get::<ImageNode>(*entity).unwrap().image,image);
            }
            let glyphs: Vec<_> = app.world_mut().query::<(&RibbonGlyph,&ImageNode)>().iter(app.world())
                .map(|(glyph,image)|(glyph.owner,glyph.icon,image.color)).collect();
            for (owner,icon,color) in glyphs {
                let expected = if owner == decoration { explicit }
                    else if owner == buttons[0].0 { theme.ink }
                    else if owner == buttons[1].0 { theme.mute }
                    else if owner == buttons[2].0 { if icon == Icon::Chevron {Color::WHITE.with_alpha(0.7)} else {Color::WHITE} }
                    else {Color::srgb_u8(224,120,120)};
                assert_eq!(color,expected,"{icon:?} retained the wrong theme ink");
            }
            app.world_mut().get_mut::<InterfaceControl>(buttons[0].0).unwrap().disabled = true;
            app.update();
            let image = app.world_mut().query::<(&RibbonGlyph,&ImageNode)>().iter(app.world())
                .find(|(glyph,_)|glyph.owner==buttons[0].0).unwrap().1;
            assert_eq!(image.color,css_mix(theme.mute,theme.header,0.4));
            app.world_mut().get_mut::<InterfaceControl>(buttons[0].0).unwrap().disabled = false;
        }
        assert_eq!(app.world_mut().query::<&ImageNode>().iter(app.world()).count(),original.len());
    }

    #[test]
    fn translated_caption_reuses_control_and_stays_inside_two_line_cell() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<ViewportUiAssets>();
        let theme = ViewportUiTheme::from_palette(&crate::native_viewport::ViewportPalette::default());
        let camera = world.spawn_empty().id();
        let control = spawn_button(&mut world.commands(),camera,node(0.,0.,48.),
            InterfaceControl::button("test","Offset Plane"),theme,&ViewportUiAssets::default());
        world.flush();
        decorate(&mut world,control,Icon::OffsetPlane);
        let binding = world.get::<InterfaceControl>(control).unwrap().clone();
        let label = world.get::<InterfaceLabel>(control).unwrap().0;
        let count = world.entities().len();
        for text in ["Abstandsebene","Rechteckiges Muster","偏移平面","Offset Plane"] {
            caption(&mut world,control,text);
            let bounds = world.get::<Node>(label).unwrap();
            assert_eq!(bounds.width,percent(100.));
            assert_eq!(bounds.height,px(16.));
            assert_eq!(bounds.overflow,Overflow::clip());
            assert_eq!(world.get::<TextLayout>(label).unwrap().linebreak,LineBreak::WordOrCharacter);
            assert_eq!(world.get::<LineHeight>(label),Some(&LineHeight::Px(8.)));
            assert_eq!(world.get::<RibbonButton>(control).unwrap().label(),text);
            assert_eq!(world.get::<InterfaceLabel>(control).unwrap().0,label);
            assert_eq!(world.get::<InterfaceControl>(control).unwrap(),&binding);
            assert_eq!(world.entities().len(),count);
        }
    }

    #[test]
    fn shared_vectors_remain_open_and_transparent_when_tinted_or_disabled() {
        for icon in [
            Icon::Extrude,
            Icon::Sketch,
            Icon::Line,
            Icon::MidpointLine,
            Icon::Rectangle,
            Icon::Circle,
            Icon::Arc,
            Icon::Slot,
            Icon::Point,
            Icon::Spline,
            Icon::Finish,
            Icon::Cancel,
            Icon::Chevron,
            Icon::Gamepad,
        ] {
            let image = rasterize(icon, 96);
            let data = image.data.unwrap();
            assert_eq!(data[3], 0, "{icon:?} background must be transparent");
            assert!(
                data.chunks_exact(4).any(|p| p[3] > 0),
                "{icon:?} must have visible strokes"
            );
        }
        let rectangle = rasterize(Icon::Rectangle, 96).data.unwrap();
        assert_eq!(
            rectangle[(48 * 96 + 48) * 4 + 3],
            0,
            "Do not fill an outlined profile"
        );
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .insert_resource(ViewportUiAssets::default())
            .add_systems(Update, update_glyphs);
        let theme =
            ViewportUiTheme::from_palette(&crate::native_viewport::ViewportPalette::default());
        let camera = app.world_mut().spawn_empty().id();
        let button = spawn_button(
            &mut app.world_mut().commands(),
            camera,
            node(0., 0., 48.),
            InterfaceControl::button("solid/build", "Extrude"),
            theme,
            &ViewportUiAssets::default(),
        );
        app.world_mut().flush();
        decorate(app.world_mut(), button, Icon::Extrude);
        let count = app.world().entities().len();
        decorate(app.world_mut(), button, Icon::Extrude);
        assert_eq!(app.world().entities().len(), count);
        for disabled in [true, false] {
            app.world_mut()
                .get_mut::<InterfaceControl>(button)
                .unwrap()
                .disabled = disabled;
            app.update();
            let image = app
                .world_mut()
                .query::<&ImageNode>()
                .single(app.world())
                .unwrap();
            assert_eq!(
                image.color,
                if disabled {
                    css_mix(theme.mute, theme.header, 0.4)
                } else {
                    theme.ink
                }
            );
        }
    }
}
