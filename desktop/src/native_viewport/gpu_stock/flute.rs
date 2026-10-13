//! Conservative reach proof for the one-height removal field.

/// The field removes everything above a stamped cutter surface. That is safe
/// only when the cutter's upper cap reaches above all retained stock throughout
/// the segment. `stock_top` includes the frame's margin above its actual stock
/// top, deliberately rejecting some safe boundary cases rather than clipping
/// material touched only by the shank.
pub(super) fn covers_stock(stock_top: f32, tip_heights: [f32; 2], flute_length: f64) -> bool {
    if !stock_top.is_finite()
        || tip_heights.iter().any(|height| !height.is_finite())
        || !flute_length.is_finite()
        || flute_length <= 0.0
    {
        return false;
    }
    let upper_cap = f64::from(tip_heights[0].min(tip_heights[1])) + flute_length;
    upper_cap.is_finite() && upper_cap >= f64::from(stock_top)
}

#[cfg(test)]
mod tests {
    use super::covers_stock;

    #[test]
    fn short_flute_preserves_material_above_its_upper_cap() {
        assert!(!covers_stock(11.0, [8.0, 8.0], 1.0));
        assert!(covers_stock(11.0, [8.0, 8.0], 20.0));
    }

    #[test]
    fn stock_margin_deliberately_rejects_a_physical_top_boundary() {
        assert!(!covers_stock(11.0, [8.0, 8.0], 2.0));
        assert!(!covers_stock(11.0, [8.0, 8.0], 2.999));
        assert!(covers_stock(11.0, [8.0, 8.0], 3.0));
    }

    #[test]
    fn both_tip_endpoints_must_reach_above_stock() {
        assert!(!covers_stock(11.0, [10.0, 8.0], 2.0));
        assert!(!covers_stock(11.0, [8.0, 10.0], 2.0));
        assert!(covers_stock(11.0, [8.0, 10.0], 3.0));
    }

    #[test]
    fn invalid_caps_do_not_enable_removal() {
        for top in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!covers_stock(top, [8.0, 8.0], 20.0));
        }
        for tip in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!covers_stock(11.0, [tip, 8.0], 20.0));
            assert!(!covers_stock(11.0, [8.0, tip], 20.0));
        }
        for flute in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            assert!(!covers_stock(11.0, [8.0, 8.0], flute));
        }
    }
}
