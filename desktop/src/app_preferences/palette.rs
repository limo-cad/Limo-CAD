//! Product appearance shared by the Bevy desktop and browser host.
use super::ResolvedTheme;
use crate::native_viewport::ViewportPalette;

const fn rgb(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 255) as f32 / 255.,
        ((hex >> 8) & 255) as f32 / 255.,
        (hex & 255) as f32 / 255.,
    ]
}

static DARK: ViewportPalette = ViewportPalette {
    background: rgb(0x2a2d33),
    panel: rgb(0x23262b),
    header: rgb(0x2b2e35),
    ui_edge: rgb(0x3a3e46),
    ink: rgb(0xd7dce2),
    mute: rgb(0x9aa0a8),
    accent: rgb(0x7463d8),
    grid_fine: rgb(0x3a3f47),
    grid_major: rgb(0x4d545f),
    body: rgb(0x8b9bac),
    body_selected: rgb(0xa96725),
    body_tool: rgb(0xb58a43),
    body_selected_edge: rgb(0xffd000),
    face_hover: rgb(0x238a9d),
    face_selected: rgb(0xcf7715),
    edge: rgb(0x29333d),
    edge_hover: rgb(0x00f5ff),
    edge_selected: rgb(0xffd000),
    pick_halo: rgb(0xffffff),
    origin_plane_xy: rgb(0x57a8ff),
    origin_plane_xz: rgb(0x55c978),
    origin_plane_yz: rgb(0xff7078),
    active_sketch: rgb(0x86a9c7),
    defined_sketch: rgb(0xe8e9ec),
    hover: rgb(0x00f5ff),
    selection: rgb(0xffd000),
    constraint_related: rgb(0x3ecf9a),
    finished_sketch: rgb(0x86a9c7),
    finished_sketch_point: rgb(0x86a9c7),
    finished_sketch_point_outline: rgb(0x15191f),
    preview: rgb(0x8fc4ff),
    dimension: rgb(0xaecb1e),
    projected: rgb(0xc08cf5),
};

static LIGHT: ViewportPalette = ViewportPalette {
    background: rgb(0xdce3ea),
    panel: rgb(0xf4f6f8),
    header: rgb(0xe8ecf1),
    ui_edge: rgb(0xc5ccd5),
    ink: rgb(0x252b32),
    mute: rgb(0x69737f),
    accent: rgb(0x6654c7),
    grid_fine: rgb(0xc5ced7),
    grid_major: rgb(0xaab6c2),
    body: rgb(0x9fb3c5),
    body_selected: rgb(0xc35d2a),
    body_tool: rgb(0xd2a04b),
    body_selected_edge: rgb(0xb83200),
    face_hover: rgb(0x2e72c5),
    face_selected: rgb(0xc34f18),
    edge: rgb(0x43515e),
    edge_hover: rgb(0x004fd8),
    edge_selected: rgb(0xb83200),
    pick_halo: rgb(0x17212b),
    origin_plane_xy: rgb(0x0b63b6),
    origin_plane_xz: rgb(0x257942),
    origin_plane_yz: rgb(0xb5323a),
    active_sketch: rgb(0x38566a),
    defined_sketch: rgb(0x252b32),
    hover: rgb(0x004fd8),
    selection: rgb(0xb83200),
    constraint_related: rgb(0x0f8f6b),
    finished_sketch: rgb(0x38566a),
    finished_sketch_point: rgb(0x38566a),
    finished_sketch_point_outline: rgb(0xffffff),
    preview: rgb(0x147fbe),
    dimension: rgb(0x344600),
    projected: rgb(0x7b3fc4),
};

pub(crate) fn viewport_palette(theme: ResolvedTheme) -> &'static ViewportPalette {
    match theme {
        ResolvedTheme::Light => &LIGHT,
        ResolvedTheme::Dark => &DARK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_body_colors_and_shared_interaction_colors_are_preserved() {
        assert_eq!(DARK.body, rgb(0x8b9bac));
        assert_eq!(LIGHT.body, rgb(0x9fb3c5));
        assert_ne!(DARK.body, ViewportPalette::default().body);
        for theme in [ResolvedTheme::Light, ResolvedTheme::Dark] {
            let palette = viewport_palette(theme);
            assert_eq!(palette.selection, palette.edge_selected);
            assert_eq!(palette.active_sketch, palette.finished_sketch);
            assert_eq!(palette.hover, palette.edge_hover);
        }
    }
}
