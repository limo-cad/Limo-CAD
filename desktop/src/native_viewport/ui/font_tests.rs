use super::*;
use bevy::{
    asset::AssetPlugin,
    text::{EditableText, TextLayoutInfo, TextPlugin},
    ui::ComputedUiRenderTargetInfo,
};

fn font_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), TextPlugin))
        .init_resource::<Assets<Image>>()
        .add_systems(Startup, load_system_font);
    app.update();
    app
}

fn shape(app: &mut App, value: &str, font: TextFont) -> Vec<(u64, u32, u32, f32)> {
    let entity = app
        .world_mut()
        .spawn((
            EditableText::new(value),
            font,
            ComputedUiRenderTargetInfo::default(),
            TextLayoutInfo::default(),
        ))
        .id();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_styles)
        .unwrap();
    app.world_mut()
        .run_system_cached(bevy::ui::widget::update_editable_text_layout)
        .unwrap();
    let editable = app.world().get::<EditableText>(entity).unwrap();
    let layout = editable.editor().try_layout().unwrap();
    layout
        .lines()
        .flat_map(|line| line.runs())
        .flat_map(|run| {
            let font = run.font().data.id();
            let index = run.font().index;
            run.clusters()
                .flat_map(|cluster| cluster.glyphs())
                .map(move |glyph| (font, index, glyph.id, glyph.advance))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn native_font_fallback_preserves_primary_latin_glyphs_and_metrics() {
    let mut app = font_app();
    let assets = app.world().resource::<ViewportUiAssets>().clone();
    let theme = ViewportUiTheme::from_palette(&default());
    for weight in [FontWeight::NORMAL, FontWeight::SEMIBOLD] {
        let font = theme.text(&assets, 13., weight);
        let mut primary = font.clone();
        primary.font = if weight == FontWeight::SEMIBOLD {
            assets.semibold.clone().or(assets.font.clone())
        } else {
            assets.font.clone()
        }
        .unwrap_or_default()
        .into();
        let expected = shape(&mut app, "Untitled CAD 123", primary);
        let actual = shape(&mut app, "Untitled CAD 123", font);
        assert!(!expected.is_empty());
        assert_eq!(actual, expected);
    }
}

#[test]
#[ignore = "platform font check: requires installed CJK and emoji fallback fonts"]
fn native_font_fallback_shapes_cjk_and_emoji_without_missing_glyphs() {
    let mut app = font_app();
    let assets = app.world().resource::<ViewportUiAssets>().clone();
    let font = ViewportUiTheme::from_palette(&default()).text(&assets, 13., FontWeight::NORMAL);
    for sample in ["Café", "零件", "Ω", "🦀"] {
        let glyphs = shape(&mut app, sample, font.clone());
        assert!(!glyphs.is_empty(), "No glyphs were shaped for {sample}");
        assert!(
            glyphs.iter().all(|(_, _, id, _)| *id != 0),
            "Missing glyph in {sample}: {glyphs:?}"
        );
    }
}

#[test]
#[ignore = "platform font check: requires an installed technical-symbol fallback face"]
fn native_font_fallback_shapes_drawing_symbols_without_missing_glyphs() {
    let mut app = font_app();
    let assets = app.world().resource::<ViewportUiAssets>().clone();
    let font = ViewportUiTheme::from_palette(&default()).text(&assets, 13., FontWeight::NORMAL);
    for sample in [
        "⌀",
        "⌴",
        "⌵",
        "↧",
        "⌖",
        "⌭",
        "⌯",
        "▱",
        "⌒",
        "⌓",
        "⌢",
        "Ⓜ\u{fe0e}",
        "Ⓛ",
        "Ⓢ",
    ] {
        let glyphs = shape(&mut app, sample, font.clone());
        assert!(!glyphs.is_empty(), "No glyphs were shaped for {sample}");
        assert!(
            glyphs.iter().all(|(_, _, id, _)| *id != 0),
            "Missing drawing glyph in {sample}: {glyphs:?}"
        );
    }
    let actual = shape(&mut app, "Ⓜ\u{fe0e}", font);
    for (font_id, index, _, _) in actual {
        let fonts = app.world().resource::<Assets<Font>>();
        let selected = fonts
            .iter()
            .find(|(_, font)| font.data.id() == font_id)
            .unwrap()
            .1;
        let bytes: &[u8] = selected.data.as_ref();
        let u16_at =
            |offset| u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
        let u32_at =
            |offset| u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let face = if bytes.starts_with(b"ttcf") {
            u32_at(12 + index as usize * 4)
        } else {
            0
        };
        for table in 0..u16_at(face + 4) {
            let tag = &bytes[face + 12 + table * 16..face + 16 + table * 16];
            assert!(
                ![b"COLR", b"CBDT", b"sbix", b"SVG "]
                    .iter()
                    .any(|color_tag| tag == *color_tag),
                "Material condition selected a color/emoji face"
            );
        }
    }
}
