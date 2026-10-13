//! Native preview ownership is explicit: product-colored sketch overlays read
//! the current palette at draw time, while imported and CAM colors stay exact.
use super::ViewportPalette;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ViewportColorRole {
    #[default]
    Explicit,
    SketchPreview,
    SketchDimension,
}

impl ViewportColorRole {
    pub(crate) fn resolve(self, explicit: [f32; 4], palette: &ViewportPalette) -> [f32; 4] {
        let rgb = match self {
            Self::Explicit => return explicit,
            Self::SketchPreview => palette.preview,
            Self::SketchDimension => palette.dimension,
        };
        [rgb[0], rgb[1], rgb[2], explicit[3]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_preferences::{palette::viewport_palette, ResolvedTheme},
        native_viewport::{ViewportLineLayer, ViewportPointLayer},
    };

    #[test]
    fn retained_native_roles_follow_canonical_palette_without_replacing_geometry_or_alpha() {
        let line = ViewportLineLayer {
            color: [0.2, 0.3, 0.4, 0.68],
            color_role: ViewportColorRole::SketchPreview,
            segments: vec![0., 0., 0., 20., 10., 0.].into(),
            ..Default::default()
        };
        let point = ViewportPointLayer {
            color: [0.2, 0.3, 0.4, 0.4],
            color_role: ViewportColorRole::SketchDimension,
            positions: vec![20., 10., 0.].into(),
            ..Default::default()
        };
        let dark = viewport_palette(ResolvedTheme::Dark);
        let light = viewport_palette(ResolvedTheme::Light);
        for palette in [dark, light, dark] {
            assert_eq!(
                line.color_role.resolve(line.color, palette),
                [
                    palette.preview[0],
                    palette.preview[1],
                    palette.preview[2],
                    0.68
                ]
            );
            assert_eq!(
                point.color_role.resolve(point.color, palette),
                [
                    palette.dimension[0],
                    palette.dimension[1],
                    palette.dimension[2],
                    0.4
                ]
            );
            assert_eq!(line.segments.as_slice(), [0., 0., 0., 20., 10., 0.]);
            assert_eq!(point.positions.as_slice(), [20., 10., 0.]);
        }
        assert_ne!(dark.preview, light.preview);
        assert_ne!(dark.dimension, light.dimension);
    }

    #[test]
    fn legacy_and_explicit_json_colors_never_acquire_native_theme_roles() {
        let explicit = [0.21, 0.45, 0.8, 0.37];
        for extra in ["", ",\"colorRole\":\"SketchPreview\""] {
            let json = format!("{{\"color\":[0.21,0.45,0.8,0.37]{extra}}}");
            let line: ViewportLineLayer = serde_json::from_str(&json).unwrap();
            let point: ViewportPointLayer = serde_json::from_str(&json).unwrap();
            for theme in [ResolvedTheme::Dark, ResolvedTheme::Light] {
                let palette = viewport_palette(theme);
                assert_eq!(line.color_role, ViewportColorRole::Explicit);
                assert_eq!(point.color_role, ViewportColorRole::Explicit);
                assert_eq!(line.color_role.resolve(line.color, palette), explicit);
                assert_eq!(point.color_role.resolve(point.color, palette), explicit);
            }
        }
    }
}
