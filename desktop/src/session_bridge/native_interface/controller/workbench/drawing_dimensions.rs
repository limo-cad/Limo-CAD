//! Native painter adapter for the shared paper-space dimension layout.
use super::{Label, Segment};
use limo_cad_sketch::{DrawingSheetStyleDto, DrawingStandard};

fn stroke(start: [f64; 2], end: [f64; 2], width: f64, arrow: bool) -> Segment {
    Segment {
        x1: start[0] as f32,
        y1: start[1] as f32,
        x2: end[0] as f32,
        y2: end[1] as f32,
        hidden: false,
        width_mm: width as f32,
        arrow,
        ink: super::Ink::Drawing,
    }
}

pub(super) fn layout(
    first: [f64; 2],
    second: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
    text: String,
    style: &DrawingSheetStyleDto,
    standard: DrawingStandard,
) -> (Vec<Segment>, Label) {
    let layout = limo_cad_occt::drawing_presentation::linear::layout(
        first, second, start, end, &text, style, standard,
    );
    let angle = layout.text_angle;
    let label = Label {
        align: Default::default(),
        text,
        x: layout.text_center[0] as f32,
        y: layout.text_center[1] as f32,
        angle: angle as f32,
        width_mm: layout.text_width as f32,
        height_mm: (style.text_height_mm * 1.18 + 1.5) as f32,
        text_height_mm: style.text_height_mm as f32,
        mask: layout.mask,
        ink: super::Ink::Drawing,
    };
    let [first, second] = layout.extensions;
    let [a, b] = layout.arrows;
    (
        vec![
            stroke(first[0], first[1], style.extension.width_mm, false),
            stroke(second[0], second[1], style.extension.width_mm, false),
            stroke(
                layout.shaft[0],
                layout.shaft[1],
                style.dimension.width_mm,
                false,
            ),
            stroke(a[0], a[1], 0., true),
            stroke(b[0], b[1], 0., true),
        ],
        label,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminals_use_paper_size_and_extensions_leave_geometry_clear() {
        let style = DrawingSheetStyleDto::default();
        let (lines, label) = layout(
            [0., 0.],
            [40., 0.],
            [0., 12.],
            [40., 12.],
            "40.00".into(),
            &style,
            DrawingStandard::Iso,
        );
        assert_eq!((lines[0].y1, lines[0].y2), (1., 13.2));
        assert_eq!(lines.iter().filter(|line| line.arrow).count(), 2);
        assert_eq!((lines[3].x1, lines[3].x2), (0., 2.5));
        assert_eq!((lines[4].x1, lines[4].x2), (40., 37.5));
        assert!(!label.mask && label.y < 12.);
        assert_eq!(lines[2].width_mm, style.dimension.width_mm as f32);
        let mut style = style;
        style.arrow_size_mm = 4.;
        let (lines, label) = layout(
            [0., 0.],
            [40., 0.],
            [0., 12.],
            [40., 12.],
            "40.00".into(),
            &style,
            DrawingStandard::Ansi,
        );
        assert_eq!(lines[3].x2, 4.);
        assert!(
            label.mask,
            "ANSI inside values interrupt the dimension line"
        );
    }
    #[test]
    fn narrow_vertical_span_moves_terminals_and_value_outside() {
        let style = DrawingSheetStyleDto::default();
        let (lines, label) = layout(
            [0., 0.],
            [0., 6.],
            [8., 0.],
            [8., 6.],
            "6.00 mm".into(),
            &style,
            DrawingStandard::Ansi,
        );
        assert_eq!((lines[3].y1, lines[3].y2), (0., -2.5));
        assert_eq!((lines[4].y1, lines[4].y2), (6., 8.5));
        assert!(lines[2].y1 < 0. && lines[2].y2 > 8.5);
        assert!(label.y > 8.5 && !label.mask);
        assert!((label.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }
}
