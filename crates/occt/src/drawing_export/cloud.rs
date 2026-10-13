//! Revision clouds are paper annotations, independent of view topology/units.
use super::{graphics::Graphics, *};
use crate::drawing_presentation::{cloud, text};

/// Clip only revision-cloud stroke centerlines to the paper rectangle. This
/// mirrors native paper clipping without moving or clamping saved vertices.
fn clip(a: P, b: P, size: P) -> Option<[P; 2]> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let mut lo = 0_f64;
    let mut hi = 1_f64;
    for (p, q) in [
        (-d[0], a[0]),
        (d[0], size[0] - a[0]),
        (-d[1], a[1]),
        (d[1], size[1] - a[1]),
    ] {
        if p.abs() < 1e-14 {
            if q < 0. {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0. {
                lo = lo.max(t)
            } else {
                hi = hi.min(t)
            };
            if lo > hi {
                return None;
            }
        }
    }
    if lo == 0. && hi == 1. {
        return Some([a, b]);
    }
    (hi - lo > 1e-14).then(|| {
        [
            [
                (a[0] + d[0] * lo).clamp(0., size[0]),
                (a[1] + d[1] * lo).clamp(0., size[1]),
            ],
            [
                (a[0] + d[0] * hi).clamp(0., size[0]),
                (a[1] + d[1] * hi).clamp(0., size[1]),
            ],
        ]
    })
}

pub(super) fn draw(
    size: P,
    revision: &str,
    points: &[P],
    budget: &mut PaperGraphicsBudget,
) -> Result<Vec<PaperPrimitive>, String> {
    let plan = cloud::Cloud::new(points)?;
    budget.work(plan.work())?;
    let label_bytes = revision
        .len()
        .checked_add(4)
        .ok_or("Revision cloud text size overflow")?;
    budget.work(label_bytes as u64)?;
    budget.scratch(
        label_bytes
            .checked_add(513 * std::mem::size_of::<P>())
            .ok_or("Revision cloud scratch overflow")?,
    )?;
    let label = format!("REV {revision}");
    let first_baseline = plan.caption_baseline(&label);
    for (row, line) in label.lines().enumerate() {
        let baseline = [
            first_baseline[0],
            first_baseline[1] + row as f64 * cloud::TEXT_HEIGHT_MM * 1.25,
        ];
        let bounds = text::label_bounds(baseline, line, cloud::TEXT_HEIGHT_MM, 1.);
        if bounds[0] < 0. || bounds[1] < 0. || bounds[2] > size[0] || bounds[3] > size[1] {
            return Err("Revision cloud caption extends outside the sheet; move its saved vertices before export".into());
        }
    }
    let style = DrawingLineStyleDto {
        width_mm: cloud::STROKE_MM,
        dash_mm: vec![],
    };
    let mut out = Graphics::new(budget);
    for arc in plan.arcs() {
        let points = arc.points();
        for pair in points.windows(2) {
            if let Some(segment) = clip(pair[0], pair[1], size) {
                out.line(&segment, "REVISION", &style)?;
            }
        }
    }
    for (row, line) in label.lines().enumerate() {
        let baseline = [
            first_baseline[0],
            first_baseline[1] + row as f64 * cloud::TEXT_HEIGHT_MM * 1.25,
        ];
        out.label(baseline, line, cloud::TEXT_HEIGHT_MM, false)?;
    }
    let mut primitives = out.finish();
    for primitive in &mut primitives {
        if let PaperPrimitive::Text { layer, .. } = primitive {
            *layer = "REVISION";
        }
    }
    Ok(primitives)
}
