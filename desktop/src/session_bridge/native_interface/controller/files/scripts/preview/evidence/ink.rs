//! Measure visible text coverage, including antialiased edges, in linear light.
//! The label backplate is opaque when the completed-frame gate samples it.

pub(super) const MINIMUM_INK: f32 = 8.;
const MINIMUM_COVERAGE: f32 = 0.25;
const MAX_COLOR_ERROR: f32 = 0.01;
const MAX_FOREGROUND_ERROR: f32 = 0.02;

pub(super) struct Ink {
    background: [f32; 3],
    delta: [f32; 3],
    squared_distance: f32,
}

fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

impl Ink {
    pub(super) fn new(foreground: [f32; 3], background: [f32; 3]) -> Self {
        let valid = foreground
            .into_iter()
            .chain(background)
            .all(|value| value.is_finite() && (0. ..=1.).contains(&value));
        if !valid {
            return Self {
                background: [0.; 3],
                delta: [0.; 3],
                squared_distance: 0.,
            };
        }
        let foreground = foreground.map(linear);
        let background = background.map(linear);
        let delta = std::array::from_fn(|axis| foreground[axis] - background[axis]);
        let squared_distance = delta.into_iter().map(|value| value * value).sum();
        Self {
            background,
            delta,
            squared_distance,
        }
    }

    /// Return equivalent fully covered pixels, rather than counting only
    /// samples close to the solid foreground color. Samples inconsistent with
    /// the expected foreground/background mixture contribute nothing.
    pub(super) fn coverage(&self, pixel: [u8; 3]) -> f32 {
        if self.squared_distance <= f32::EPSILON {
            return 0.;
        }
        let sample = pixel.map(|value| linear(f32::from(value) / 255.));
        let coverage = ((0..3)
            .map(|axis| (sample[axis] - self.background[axis]) * self.delta[axis])
            .sum::<f32>()
            / self.squared_distance)
            .clamp(0., 1.);
        if coverage < MINIMUM_COVERAGE
            || (0..3).any(|axis| {
                let expected = self.background[axis] + coverage * self.delta[axis];
                let error = (sample[axis] - expected).abs();
                // Absolute tolerance alone hides the wrong ink hue at low coverage.
                // Also bound the error of the inferred full-coverage foreground.
                error > MAX_COLOR_ERROR || error > MAX_FOREGROUND_ERROR * coverage
            })
        {
            0.
        } else {
            coverage
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(value: [u8; 3]) -> [f32; 3] {
        value.map(|channel| f32::from(channel) / 255.)
    }

    fn composite(foreground: [u8; 3], background: [u8; 3], coverage: f32) -> [u8; 3] {
        let foreground = rgb(foreground).map(linear);
        let background = rgb(background).map(linear);
        std::array::from_fn(|axis| {
            let value = coverage * foreground[axis] + (1. - coverage) * background[axis];
            let encoded = if value <= 0.0031308 {
                value * 12.92
            } else {
                1.055 * value.powf(1. / 2.4) - 0.055
            };
            (encoded * 255.).round() as u8
        })
    }

    #[test]
    fn native_antialiased_light_label_samples_retain_visible_coverage() {
        let ink = Ink::new(rgb([52, 70, 0]), rgb([220, 227, 234]));
        // Original native demo-175-en.png, run41028744703, label32.00.
        // The old full-color proximity gate rejected these visible samples.
        for pixel in [
            [176, 183, 185],
            [170, 177, 178],
            [186, 194, 196],
            [161, 168, 170],
            [99, 109, 92],
            [100, 111, 94],
        ] {
            assert!(ink.coverage(pixel) >= MINIMUM_COVERAGE, "{pixel:?}");
        }
    }

    #[test]
    fn both_themes_reject_blank_grid_and_wrong_color_samples() {
        for (foreground, background, excluded) in [
            (
                [52, 70, 0],
                [220, 227, 234],
                [[197, 206, 215], [170, 182, 194], [123, 63, 196]],
            ),
            (
                [174, 203, 30],
                [42, 45, 51],
                [[58, 63, 71], [77, 84, 95], [192, 140, 245]],
            ),
        ] {
            let ink = Ink::new(rgb(foreground), rgb(background));
            assert!((ink.coverage(foreground) - 1.).abs() < 1e-5);
            for pixel in excluded.into_iter().chain([background, [180, 40, 40]]) {
                assert_eq!(ink.coverage(pixel), 0., "{pixel:?}");
            }
        }
    }

    #[test]
    fn antialiased_neutral_text_does_not_count_as_dimension_ink() {
        let background = [220, 227, 234];
        let ink = Ink::new(rgb([52, 70, 0]), rgb(background));
        // This quantized UI-ink blend passed the absolute-only error bound.
        assert_eq!(ink.coverage([194, 201, 207]), 0.);
        for foreground in [[37, 43, 50], [80; 3], [128; 3]] {
            for percent in 25..=100 {
                let pixel = composite(foreground, background, percent as f32 / 100.);
                assert_eq!(
                    ink.coverage(pixel),
                    0.,
                    "{foreground:?}, {percent}%, {pixel:?}"
                );
            }
        }
    }

    #[test]
    fn quantized_dimension_blends_retain_visible_coverage_in_both_themes() {
        for (foreground, background) in [
            ([52, 70, 0], [220, 227, 234]),
            ([174, 203, 30], [42, 45, 51]),
        ] {
            let ink = Ink::new(rgb(foreground), rgb(background));
            for expected in [0.3, 0.5, 0.75, 1.] {
                let pixel = composite(foreground, background, expected);
                let actual = ink.coverage(pixel);
                assert!(
                    actual >= MINIMUM_COVERAGE && (actual - expected).abs() < 0.02,
                    "{foreground:?}, {expected}, {pixel:?}, {actual}"
                );
            }
        }
    }

    #[test]
    fn faint_noise_and_missing_or_insufficient_ink_do_not_qualify() {
        let ink = Ink::new(rgb([52, 70, 0]), rgb([220, 227, 234]));
        assert_eq!(ink.coverage([211, 218, 224]), 0.);
        assert_eq!(ink.coverage([37, 43, 50]), 0.);
        let solid = ink.coverage([52, 70, 0]);
        for pixels in [0, 7, 8] {
            let total = (0..pixels).map(|_| solid).sum::<f32>();
            assert!(total <= MINIMUM_INK);
        }
        assert!((0..9).map(|_| solid).sum::<f32>() > MINIMUM_INK);
    }

    #[test]
    fn invalid_or_contrast_free_palettes_cannot_generate_ink() {
        for foreground in [[0.5; 3], [f32::NAN; 3], [2.; 3]] {
            let ink = Ink::new(foreground, [0.5; 3]);
            assert_eq!(ink.coverage([0, 0, 0]), 0.);
        }
    }
}
