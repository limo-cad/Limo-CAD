//! Allocate disjoint screen footprints without changing saved sketch anchors.
//! Include rotation and the button's focus outline in every reserved footprint.

const OUTLINE: f32 = 3.;
const GAP: f32 = 4.;
const MARGIN: f32 = 4.;

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: [f32; 2],
    max: [f32; 2],
}

impl Bounds {
    fn at(center: [f32; 2], half: [f32; 2]) -> Self {
        Self {
            min: [center[0] - half[0], center[1] - half[1]],
            max: [center[0] + half[0], center[1] + half[1]],
        }
    }

    fn conflicts(self, other: Self) -> bool {
        (0..2).all(|axis| {
            self.min[axis] < other.max[axis] + GAP && self.max[axis] + GAP > other.min[axis]
        })
    }
}

pub(super) struct Layout {
    size: [f32; 2],
    // Sorted by the left edge for the horizontal free-space sweep.
    occupied: Vec<Bounds>,
}

impl Layout {
    pub(super) fn new(size: [f32; 2]) -> Self {
        Self {
            size,
            occupied: Vec::new(),
        }
    }

    /// Keep the preferred center when possible; otherwise choose the nearest
    /// free center inside the canvas. A completely full canvas omits the box
    /// rather than rendering overlapping or clipped controls.
    pub(super) fn place(
        &mut self,
        preferred: [f32; 2],
        size: [f32; 2],
        angle: f32,
    ) -> Option<[f32; 2]> {
        if self
            .size
            .into_iter()
            .chain(preferred)
            .chain(size)
            .any(|v| !v.is_finite())
            || size.iter().any(|v| *v <= 0.)
            || !angle.is_finite()
        {
            return None;
        }
        let (sin, cos) = angle.sin_cos();
        let half = [size[0] * 0.5 + OUTLINE, size[1] * 0.5 + OUTLINE];
        let half = [
            cos.abs() * half[0] + sin.abs() * half[1],
            sin.abs() * half[0] + cos.abs() * half[1],
        ];
        let min = [MARGIN + half[0], MARGIN + half[1]];
        let max = [
            self.size[0] - MARGIN - half[0],
            self.size[1] - MARGIN - half[1],
        ];
        if (0..2).any(|axis| min[axis] > max[axis]) {
            return None;
        }
        let preferred = [
            preferred[0].clamp(min[0], max[0]),
            preferred[1].clamp(min[1], max[1]),
        ];
        let mut center = preferred;
        if self
            .occupied
            .iter()
            .any(|other| Bounds::at(center, half).conflicts(*other))
        {
            // The closest free point lies at the preferred y or at an edge of
            // an expanded obstacle. Sweep those rows, merging blocked x spans.
            // The small extra clearance absorbs floating-point edge rounding.
            let clearance = GAP + 0.25;
            let mut rows = vec![preferred[1], min[1], max[1]];
            for other in &self.occupied {
                rows.extend([
                    other.min[1] - half[1] - clearance,
                    other.max[1] + half[1] + clearance,
                ]);
            }
            rows.retain(|y| *y >= min[1] && *y <= max[1]);
            rows.sort_by(|a, b| {
                (a - preferred[1])
                    .abs()
                    .total_cmp(&(b - preferred[1]).abs())
            });
            rows.dedup();
            let mut best = None;
            let mut distance = f32::INFINITY;
            for y in rows {
                let dy = (y - preferred[1]).powi(2);
                if dy > distance {
                    break;
                }
                let mut consider = |left: f32, right: f32| {
                    if left > right {
                        return;
                    }
                    let x = preferred[0].clamp(left, right);
                    let d = (x - preferred[0]).powi(2) + dy;
                    if d < distance {
                        distance = d;
                        best = Some([x, y]);
                    }
                };
                let mut left = min[0];
                for other in &self.occupied {
                    if y + half[1] + clearance <= other.min[1]
                        || y - half[1] - clearance >= other.max[1]
                    {
                        continue;
                    }
                    let blocked_left = other.min[0] - half[0] - clearance;
                    let blocked_right = other.max[0] + half[0] + clearance;
                    if blocked_right < left {
                        continue;
                    }
                    if blocked_left > left {
                        consider(left, blocked_left.min(max[0]));
                    }
                    left = left.max(blocked_right);
                    if left > max[0] {
                        break;
                    }
                }
                consider(left, max[0]);
            }
            center = best?;
        }
        let bounds = Bounds::at(center, half);
        if self.occupied.iter().any(|other| bounds.conflicts(*other)) {
            return None;
        }
        let index = self
            .occupied
            .partition_point(|other| other.min[0] <= bounds.min[0]);
        self.occupied.insert(index, bounds);
        Some(center)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn separated(layout: &Layout) {
        for (i, bounds) in layout.occupied.iter().enumerate() {
            assert!((0..2).all(|axis| bounds.min[axis] >= MARGIN - 0.001
                && bounds.max[axis] <= layout.size[axis] - MARGIN + 0.001));
            for other in &layout.occupied[i + 1..] {
                assert!(!bounds.conflicts(*other), "{bounds:?} overlaps {other:?}");
            }
        }
    }

    #[test]
    fn constraint_moves_clear_of_dimension_without_displacing_its_preferred_anchor() {
        let mut layout = Layout::new([360., 260.]);
        assert_eq!(
            layout.place([192., 140.], [80., 20.], 0.),
            Some([192., 140.])
        );
        let constraint = layout.place([192., 140.], [22., 22.], 0.).unwrap();
        assert_ne!(constraint, [192., 140.]);
        assert_eq!(layout.occupied.len(), 2);
        separated(&layout);
    }

    #[test]
    fn crowded_rotated_and_long_labels_stay_separate_across_zoom_and_interface_sizes() {
        for scale in [0.75, 1., 1.75] {
            for zoom in [0.05, 0.25, 1., 10.] {
                let mut layout = Layout::new([900. / scale, 650. / scale]);
                for i in 0..80 {
                    let preferred = [
                        450. / scale + (i % 5) as f32 * zoom,
                        325. / scale + (i / 5) as f32 * zoom,
                    ];
                    let (size, angle) = if i % 3 == 0 {
                        ([110., 20.], (i % 4) as f32 * std::f32::consts::FRAC_PI_4)
                    } else {
                        ([22., 22.], 0.)
                    };
                    let placed = layout.place(preferred, size, angle);
                    if i < 20 {
                        assert!(placed.is_some(), "scale {scale}, zoom {zoom}, box {i}");
                    }
                    separated(&layout);
                }
            }
        }
    }

    #[test]
    fn viewport_edges_clamp_complete_boxes_and_full_canvases_do_not_overlap() {
        let mut layout = Layout::new([110., 90.]);
        for preferred in [[0., 0.], [110., 0.], [110., 90.], [0., 90.]] {
            assert!(layout.place(preferred, [22., 22.], 0.).is_some());
        }
        let before = layout.occupied.len();
        assert!(layout.place([55., 45.], [100., 80.], 0.).is_none());
        assert_eq!(layout.occupied.len(), before);
        separated(&layout);
        let mut layout = Layout::new([40., 40.]);
        assert!(layout.place([20., 20.], [22., 22.], 0.).is_some());
        assert!(layout.place([20., 20.], [22., 22.], 0.).is_none());
        separated(&layout);
    }

    #[test]
    fn layout_is_deterministic_and_rejects_unusable_bounds() {
        let allocate = || {
            let mut layout = Layout::new([300., 200.]);
            (0..12)
                .map(|_| layout.place([150., 100.], [48., 20.], 0.))
                .collect::<Vec<_>>()
        };
        assert_eq!(allocate(), allocate());
        let mut layout = Layout::new([300., 200.]);
        assert!(layout.place([f32::NAN, 50.], [48., 20.], 0.).is_none());
        assert!(layout.place([50., 50.], [0., 20.], 0.).is_none());
        assert!(layout
            .place([50., 50.], [48., 20.], f32::INFINITY)
            .is_none());
    }
}
