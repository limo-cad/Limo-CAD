//! Resolved paper-space center markings. These are disposable geometry, not
//! document records; callers retain topology and occurrence ownership.
use super::geometry::{add, scale, sub, unit, P};

fn valid(center: P, radius: f64, extension: f64) -> bool {
    center.into_iter().all(f64::is_finite)
        && radius.is_finite()
        && radius > 0.
        && extension.is_finite()
}

/// Keep automatic view captions below resolved center ink in paper space.
/// A view without a center annotation keeps its existing caption placement.
pub fn caption_baseline(existing: f64, text_height: f64, ink_bottom: Option<f64>) -> f64 {
    ink_bottom
        .filter(|bottom| bottom.is_finite())
        .map_or(existing, |bottom| existing.max(bottom + text_height + 1.))
}

/// The circle radius has already been scaled to paper millimetres. Extension
/// is independently in paper millimetres, matching the existing drawing editor.
pub fn mark(center: P, radius: f64, extension: f64) -> Option<[[P; 2]; 2]> {
    if !valid(center, radius, extension) {
        return None;
    }
    let extent = radius + extension.max(0.);
    let result = [
        [add(center, [-extent, 0.]), add(center, [extent, 0.])],
        [add(center, [0., -extent]), add(center, [0., extent])],
    ];
    result
        .iter()
        .flatten()
        .flatten()
        .all(|x| x.is_finite())
        .then_some(result)
}

pub fn line(
    first: P,
    first_radius: f64,
    second: P,
    second_radius: f64,
    extension: f64,
) -> Option<[P; 2]> {
    if !valid(first, first_radius, extension) || !valid(second, second_radius, extension) {
        return None;
    }
    let direction = unit(sub(second, first))?;
    let result = [
        add(first, scale(direction, -first_radius - extension.max(0.))),
        add(second, scale(direction, second_radius + extension.max(0.))),
    ];
    result
        .iter()
        .flatten()
        .all(|x| x.is_finite())
        .then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_clearance_preserves_normal_baseline_and_includes_ink_and_text_height() {
        assert_eq!(caption_baseline(119., 2.5, None), 119.);
        assert_eq!(caption_baseline(119., 2.5, Some(110.)), 119.);
        assert_eq!(caption_baseline(119., 2.5, Some(117.275)), 120.775);
    }

    #[test]
    fn extension_is_paper_millimetres_and_asymmetric_radii_keep_their_centers() {
        assert_eq!(
            mark([100., 80.], 20., 3.),
            Some([[[77., 80.], [123., 80.]], [[100., 57.], [100., 103.]],])
        );
        assert_eq!(
            line([100., 80.], 20., [140., 80.], 10., 3.),
            Some([[77., 80.], [153., 80.]])
        );
        assert_eq!(
            line([140., 80.], 10., [100., 80.], 20., 3.),
            Some([[153., 80.], [77., 80.]])
        );
        assert_eq!(
            mark([100., 80.], 40., 3.).unwrap()[0],
            [[57., 80.], [143., 80.]]
        );
    }

    #[test]
    fn oblique_line_follows_the_centers_and_collapsed_or_nonfinite_geometry_is_rejected() {
        assert_eq!(
            line([10., 20.], 5., [13., 24.], 10., 0.),
            Some([[7., 16.], [19., 32.]])
        );
        assert!(line([10., 20.], 5., [10., 20.], 10., 3.).is_none());
        assert!(mark([f64::MAX, 0.], f64::MAX, 3.).is_none());
        assert!(mark([0., 0.], 1., f64::NAN).is_none());
        assert!(line([0., 0.], f64::INFINITY, [1., 0.], 1., 0.).is_none());
        assert!(mark([0., 0.], 0., 0.).is_none());
    }
}
