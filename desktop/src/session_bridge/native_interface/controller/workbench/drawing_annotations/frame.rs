//! Bounded SVG subpaths shared by annotations and the sheet frame.
use super::*;

impl CheckedArt {
    pub(in super::super) fn finish(self) -> Result<Art, String> {
        self.budget.check()?;
        Ok(self.art)
    }

    pub(in super::super) fn ready(&self) -> bool {
        self.budget.ready()
    }

    /// Check metadata before cloning/formatting it into frame labels.
    pub(in super::super) fn frame_input(&mut self, bytes: usize, rows: usize) -> bool {
        if rows > self.budget.limits.labels {
            return self
                .budget
                .reject("Drawing frame tables exceed the primitive limit; simplify the sheet");
        }
        self.budget.work(rows as u64)
            && self.budget.steps(bytes as f64 * 32.)
            && self.budget.scratch(bytes.saturating_mul(8))
    }

    /// A call is one SVG subpath. Rectangles pass all four edges together;
    /// separate title/table lines call this separately and reset the phase.
    pub(in super::super) fn styled_path(
        &mut self,
        points: &[P],
        style: &DrawingLineStyleDto,
        ink: Ink,
    ) {
        if !self.budget.path(points, style) {
            return;
        }
        if style.dash_mm.is_empty() {
            for pair in points.windows(2) {
                if !self.budget.ready() {
                    return;
                }
                self.stroke(pair[0], pair[1], style.width_mm, ink);
            }
            return;
        }
        let mut index = 0usize;
        let mut remaining = style.dash_mm[0];
        for pair in points.windows(2) {
            let delta = sub(pair[1], pair[0]);
            let distance = length(delta);
            if distance == 0. {
                continue;
            }
            if !distance.is_finite() {
                self.budget
                    .reject("Drawing line length is outside the supported range");
                return;
            }
            let direction = scale(delta, 1. / distance);
            let mut offset = 0.;
            while offset < distance {
                if !self.budget.work(1) {
                    return;
                }
                let step = remaining.min(distance - offset);
                let next = offset + step;
                if next <= offset {
                    self.budget.reject("Drawing dash precision is exhausted");
                    return;
                }
                if index.is_multiple_of(2) {
                    let a = add(pair[0], scale(direction, offset));
                    let b = add(pair[0], scale(direction, next));
                    self.segment(Segment {
                        x1: a[0] as f32,
                        y1: a[1] as f32,
                        x2: b[0] as f32,
                        y2: b[1] as f32,
                        hidden: false,
                        width_mm: style.width_mm as f32,
                        arrow: false,
                        ink,
                    });
                }
                offset = next;
                if step == remaining {
                    index += 1;
                    remaining = style.dash_mm[index % style.dash_mm.len()];
                } else {
                    remaining -= step;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_line_and_polyline_preserve_saved_dashes_below_point_zero_five() {
        let style = DrawingLineStyleDto {
            width_mm: 0.25,
            dash_mm: vec![0.01, 0.02],
        };
        let mut line = CheckedArt::default();
        line.line([0., 0.], [0.065, 0.], &style, Ink::Drawing);
        let mut polyline = CheckedArt::default();
        polyline.polyline(&[[0., 0.], [0.015, 0.], [0.065, 0.]], &style, Ink::Drawing);
        for art in [&line, &polyline] {
            assert!(art.budget.check().is_ok());
            assert_eq!(art.segments.len(), 3);
            for (actual, [start, end]) in
                art.segments
                    .iter()
                    .zip([[0., 0.01], [0.03, 0.04], [0.06, 0.065]])
            {
                assert!((actual.x1 - start).abs() < 1e-7);
                assert!((actual.x2 - end).abs() < 1e-7);
            }
        }
    }
}
