//! Measure visible text coverage, including antialiased edges, in linear light.
//! The label backplate is opaque when the completed-frame gate samples it.

pub(super) const MINIMUM_INK: f32 = 8.;
const MINIMUM_COVERAGE: f32 = 0.25;
const MAX_COLOR_ERROR: f32 = 0.01;

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
    /// samples close to the solid foreground color. Blank, low-contrast and
    /// unrelated-color pixels contribute nothing.
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
                (sample[axis] - expected).abs() > MAX_COLOR_ERROR
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
