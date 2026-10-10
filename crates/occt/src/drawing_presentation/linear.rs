//! The native dimension layout in paper millimetres, independent of its painter.
use super::geometry::{add, scale, sub, P};
use limo_cad_sketch::{DrawingSheetStyleDto, DrawingStandard};

pub struct Layout {
    pub extensions: [[P; 2]; 2],
    pub shaft: [P; 2],
    /// Tip and inward/outward base centre, at the saved paper arrow size.
    pub arrows: [[P; 2]; 2],
    pub text_baseline: P,
    pub text_center: P,
    pub text_angle: f64,
    pub text_width: f64,
    pub mask: bool,
    /// ANSI text interrupts the shaft. Renderers without text masks use these
    /// two intervals; masked native paint retains the complete shaft.
    pub unmasked_shaft: [[P; 2]; 2],
    pub unmasked_shaft_count: usize,
}

fn offset(point: P, direction: P, distance: f64) -> P {
    add(point, scale(direction, distance))
}

fn extension(anchor: P, end: P) -> [P; 2] {
    let delta = sub(end, anchor);
    let length = delta[0].hypot(delta[1]);
    if length < 1e-7 {
        return [anchor, end];
    }
    let direction = scale(delta, 1. / length);
    [
        offset(anchor, direction, 1_f64.min(length * 0.2)),
        offset(end, direction, 1.2),
    ]
}

pub fn layout(
    first: P,
    second: P,
    start: P,
    end: P,
    text: &str,
    style: &DrawingSheetStyleDto,
    standard: DrawingStandard,
) -> Layout {
    let span = (end[0] - start[0]).hypot(end[1] - start[1]);
    let direction = [(end[0] - start[0]) / span, (end[1] - start[1]) / span];
    let text_width = super::text::width(text, style.text_height_mm);
    let arrow = style.arrow_size_mm;
    let clearance = 0.8_f64.max(arrow * 0.4);
    let outside = span < text_width + 2. * arrow + 2. * clearance;
    let text_outside = span < text_width + 2. * clearance;
    let mut line_start = start;
    let mut line_end = end;
    let mut text_point = [(start[0] + end[0]) / 2., (start[1] + end[1]) / 2.];
    if outside {
        line_start = offset(start, direction, -arrow - clearance);
        line_end = offset(end, direction, arrow + clearance);
        if text_outside {
            let text_offset = arrow + 1_f64.max(arrow * 0.55) + text_width / 2.;
            line_end = offset(end, direction, text_offset + text_width / 2. + clearance);
            text_point = offset(end, direction, text_offset);
        }
    }
    let mask = standard == DrawingStandard::Ansi && !text_outside;
    let mut angle = direction[1].atan2(direction[0]);
    if angle > std::f64::consts::FRAC_PI_2 || angle < -std::f64::consts::FRAC_PI_2 {
        angle += std::f64::consts::PI;
    }
    let baseline = if mask {
        0.8
    } else {
        1.2_f64.max(style.text_height_mm * 0.22 + style.dimension.width_mm * 2.)
    };
    let sign = if outside { -1. } else { 1. };
    Layout {
        extensions: [extension(first, start), extension(second, end)],
        shaft: [line_start, line_end],
        arrows: [
            [start, offset(start, direction, arrow * sign)],
            [end, offset(end, direction, -arrow * sign)],
        ],
        text_baseline: [
            text_point[0] + angle.sin() * baseline,
            text_point[1] - angle.cos() * baseline,
        ],
        text_center: [
            text_point[0] + angle.sin() * (baseline + style.text_height_mm * 0.4),
            text_point[1] - angle.cos() * (baseline + style.text_height_mm * 0.4),
        ],
        text_angle: angle,
        text_width,
        mask,
        unmasked_shaft: if mask {
            [
                [line_start, offset(text_point, direction, -text_width / 2.)],
                [offset(text_point, direction, text_width / 2.), line_end],
            ]
        } else {
            [[line_start, line_end], [line_end, line_end]]
        },
        unmasked_shaft_count: if mask { 2 } else { 1 },
    }
}
