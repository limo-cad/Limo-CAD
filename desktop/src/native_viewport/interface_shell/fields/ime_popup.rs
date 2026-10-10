//! OS candidate-window geometry. Winit places that popup from a caret rectangle
//! in logical window coordinates, with the client origin at the top left.
//! Windows turns the rectangle into the IMM candidate exclusion area. macOS
//! reports it from `firstRectForCharacterRange`. The monitor scale factor is
//! applied when the rectangle is handed to the OS, so a DPI change is sent
//! again even when the logical caret has not moved.

use bevy::{
    ecs::schedule::ScheduleLabel,
    math::{Affine2, Rect, Vec2},
    prelude::*,
    window::PrimaryWindow,
};
use winit::dpi::{LogicalPosition, LogicalSize};

/// Rectangle in pixel space. `x` and `y` are the top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PixelRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Candidate exclusion rectangle in logical window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CandidatePopup {
    pub origin: [f32; 2],
    pub size: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ImePopupInput {
    /// Parley caret or preedit box in layout pixels, relative to the text origin.
    pub caret: PixelRect,
    /// Field content-box minimum in node-local physical pixels.
    pub content_min: Vec2,
    /// Text viewport scroll, in the same pixels as `caret`.
    pub scroll: Vec2,
    pub transform: Affine2,
    /// Field border box in node-local physical pixels.
    pub field_local: Rect,
    /// `ComputedNode::inverse_scale_factor` from the last UI layout.
    pub inverse_scale_factor: f32,
    pub ui_scale: f32,
    /// Monitor scale factor from the Bevy window (`physical = logical * scale`).
    pub monitor_scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ImePlacement {
    pub popup: CandidatePopup,
    pub field_pixels: PixelRect,
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub(super) struct ImeCandidateWindow {
    pub enabled: bool,
    pub popup: Option<CandidatePopup>,
    pub field_pixels: Option<PixelRect>,
    pub scale_factor: f32,
}

/// Logical window position of a caret that is already in physical window pixels.
pub(super) fn candidate_popup_from_caret(
    caret: PixelRect,
    scale_factor: f32,
) -> Option<CandidatePopup> {
    if !usable_scale(scale_factor) || !usable_rect(caret) {
        return None;
    }
    Some(CandidatePopup {
        origin: [caret.x / scale_factor, caret.y / scale_factor],
        size: [caret.width / scale_factor, caret.height / scale_factor],
    })
}

/// Physical pixel rectangle of a field whose layout rect is in logical pixels.
pub(super) fn field_pixel_rect(logical: PixelRect, scale_factor: f32) -> Option<PixelRect> {
    if !usable_scale(scale_factor) || !usable_rect(logical) {
        return None;
    }
    Some(PixelRect {
        x: logical.x * scale_factor,
        y: logical.y * scale_factor,
        width: logical.width * scale_factor,
        height: logical.height * scale_factor,
    })
}

pub(super) fn place_ime_popup(input: ImePopupInput) -> Option<ImePlacement> {
    if !usable_scale(input.inverse_scale_factor)
        || !usable_scale(input.ui_scale)
        || !usable_scale(input.monitor_scale)
    {
        return None;
    }
    let origin = Vec2::new(input.caret.x, input.caret.y) + input.content_min - input.scroll;
    let mut caret_local = Rect::from_corners(
        origin,
        origin + Vec2::new(input.caret.width, input.caret.height),
    );
    caret_local.min = caret_local
        .min
        .clamp(input.field_local.min, input.field_local.max);
    caret_local.max = caret_local
        .max
        .clamp(caret_local.min, input.field_local.max);
    let caret_physical = physical_at_monitor(
        map_rect(caret_local, input.transform),
        input.inverse_scale_factor,
        input.ui_scale,
        input.monitor_scale,
    )?;
    let field_logical = scale_about_origin(
        map_rect(input.field_local, input.transform),
        input.inverse_scale_factor,
    );
    Some(ImePlacement {
        popup: candidate_popup_from_caret(caret_physical, input.monitor_scale)?,
        field_pixels: field_pixel_rect(field_logical, input.monitor_scale * input.ui_scale)?,
    })
}

fn physical_at_monitor(
    laid_out: PixelRect,
    inverse_scale_factor: f32,
    ui_scale: f32,
    monitor_scale: f32,
) -> Option<PixelRect> {
    let logical = scale_about_origin(laid_out, inverse_scale_factor);
    field_pixel_rect(logical, monitor_scale * ui_scale)
}

fn map_rect(rect: Rect, transform: Affine2) -> PixelRect {
    let corners = [
        rect.min,
        Vec2::new(rect.max.x, rect.min.y),
        rect.max,
        Vec2::new(rect.min.x, rect.max.y),
    ];
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for corner in corners {
        let point = transform.transform_point2(corner);
        min = min.min(point);
        max = max.max(point);
    }
    PixelRect {
        x: min.x,
        y: min.y,
        width: max.x - min.x,
        height: max.y - min.y,
    }
}

fn scale_about_origin(rect: PixelRect, factor: f32) -> PixelRect {
    PixelRect {
        x: rect.x * factor,
        y: rect.y * factor,
        width: rect.width * factor,
        height: rect.height * factor,
    }
}

fn usable_scale(scale: f32) -> bool {
    scale.is_finite() && scale > 0.
}

fn usable_rect(rect: PixelRect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
        && rect.width >= 0.
        && rect.height >= 0.
}

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct PlaceImeCandidate;

/// Runs after Bevy copies `Window::ime_position` into winit. Bevy's copy uses a
/// fixed 10×10 physical cursor, so this call replaces it with the caret rect.
pub(super) fn place_os_candidate(
    windows: Query<Entity, With<PrimaryWindow>>,
    state: Res<ImeCandidateWindow>,
    mut pushed: Local<Option<(CandidatePopup, f32)>>,
) {
    let next = if state.enabled {
        state.popup.map(|popup| (popup, state.scale_factor))
    } else {
        None
    };
    if *pushed == next {
        return;
    }
    let Some((popup, _)) = next else {
        *pushed = None;
        return;
    };
    let Ok(entity) = windows.single() else {
        return;
    };
    let placed = bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let Some(window) = windows.get_window(entity) else {
            return false;
        };
        window.set_ime_cursor_area(
            LogicalPosition::new(f64::from(popup.origin[0]), f64::from(popup.origin[1])),
            LogicalSize::new(
                f64::from(popup.size[0].max(1.)),
                f64::from(popup.size[1].max(1.)),
            ),
        );
        true
    });
    if placed {
        *pushed = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field() -> Rect {
        Rect::from_center_size(Vec2::ZERO, Vec2::new(80., 24.))
    }

    fn input(scroll: Vec2, translation: Vec2, inverse: f32, monitor: f32) -> ImePopupInput {
        ImePopupInput {
            caret: PixelRect {
                x: 10.,
                y: 4.,
                width: 6.,
                height: 18.,
            },
            content_min: Vec2::new(-40., -12.),
            scroll,
            transform: Affine2::from_translation(translation),
            field_local: field(),
            inverse_scale_factor: inverse,
            ui_scale: 1.,
            monitor_scale: monitor,
        }
    }

    #[test]
    fn candidate_popup_position_comes_from_the_caret_rect_and_scale_factor() {
        let caret = PixelRect {
            x: 80.,
            y: 48.,
            width: 10.,
            height: 28.,
        };
        let popup = candidate_popup_from_caret(caret, 2.).unwrap();
        assert_eq!(popup.origin, [40., 24.]);
        assert_eq!(popup.size, [5., 14.]);
        let logical = PixelRect {
            x: 16.,
            y: 20.,
            width: 120.,
            height: 26.,
        };
        assert_eq!(
            field_pixel_rect(logical, 2.).unwrap(),
            PixelRect {
                x: 32.,
                y: 40.,
                width: 240.,
                height: 52.,
            }
        );
        assert!(candidate_popup_from_caret(caret, 0.).is_none());
        assert!(field_pixel_rect(logical, f32::NAN).is_none());
    }

    #[test]
    fn popup_follows_scroll_field_movement_and_monitor_scale() {
        let resting = place_ime_popup(input(Vec2::ZERO, Vec2::new(60., 42.), 1., 1.)).unwrap();
        assert_eq!(resting.popup.origin, [30., 34.]);
        assert_eq!(resting.popup.size, [6., 18.]);
        assert_eq!(
            resting.field_pixels,
            PixelRect {
                x: 20.,
                y: 30.,
                width: 80.,
                height: 24.,
            }
        );

        let scrolled =
            place_ime_popup(input(Vec2::new(12., 3.), Vec2::new(60., 42.), 1., 1.)).unwrap();
        assert_eq!(scrolled.popup.origin, [20., 31.]);

        let moved = place_ime_popup(input(Vec2::ZERO, Vec2::new(90., 50.), 1., 1.)).unwrap();
        assert_eq!(moved.popup.origin, [60., 42.]);
        assert_eq!(
            moved.field_pixels,
            PixelRect {
                x: 50.,
                y: 38.,
                width: 80.,
                height: 24.,
            }
        );

        let dpi = place_ime_popup(input(Vec2::ZERO, Vec2::new(60., 42.), 1., 2.)).unwrap();
        assert_eq!(dpi.popup.origin, [30., 34.]);
        assert_eq!(dpi.popup.size, [6., 18.]);
        assert_eq!(
            dpi.field_pixels,
            PixelRect {
                x: 40.,
                y: 60.,
                width: 160.,
                height: 48.,
            }
        );

        let mut caught_up = input(Vec2::ZERO, Vec2::new(120., 84.), 0.5, 2.);
        caught_up.caret = PixelRect {
            x: 20.,
            y: 8.,
            width: 12.,
            height: 36.,
        };
        caught_up.content_min = Vec2::new(-80., -24.);
        caught_up.field_local = Rect::from_center_size(Vec2::ZERO, Vec2::new(160., 48.));
        let caught_up = place_ime_popup(caught_up).unwrap();
        assert_eq!(caught_up.popup, dpi.popup);
        assert_eq!(caught_up.field_pixels, dpi.field_pixels);
    }
}
