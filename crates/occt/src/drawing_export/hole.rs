//! Saved hole callouts over the same exact circle and label as native paper.
use super::*;
use crate::drawing_presentation::{geometry, text};

pub(super) fn draw(
    paper: &mut Paper,
    sheet: &DrawingSheetDto,
    projections: &BTreeMap<u64, DrawingProjectionDto>,
    annotation: &DrawingAnnotationDto,
    units: limo_cad_core::UnitSystem,
) -> Result<(), String> {
    let DrawingAnnotationDto::HoleNote {
        view_id,
        feature,
        position,
        ..
    } = annotation
    else {
        unreachable!("Only hole notes are dispatched here");
    };
    let (view, projection) = view_projection(*view_id, sheet, projections)?;
    let (center, radius) = centers::circle(feature, view, projection)?;
    let direction = geometry::unit(geometry::sub(*position, center))
        .ok_or("Hole note leader coincides with its circular center")?;
    let attachment = geometry::add(center, geometry::scale(direction, radius));
    let direction = geometry::unit(geometry::sub(*position, attachment))
        .ok_or("Hole note leader has zero length")?;
    let style = &sheet.style;
    paper.line(vec![attachment, *position], "LEADER", &style.leader);
    let base = geometry::add(attachment, geometry::scale(direction, style.arrow_size_mm));
    let side = geometry::scale(geometry::normal(direction), style.arrow_size_mm * 0.3);
    paper.items.push(Primitive::Triangle {
        points: [
            attachment,
            geometry::add(base, side),
            geometry::sub(base, side),
        ],
        layer: "LEADER",
    });
    let label = text::hole(annotation, units, sheet.standard);
    let lines: Vec<_> = label
        .lines()
        .enumerate()
        .map(|(index, line)| {
            (
                line,
                [
                    position[0] + 1.2,
                    position[1] - 0.8 + index as f64 * style.text_height_mm * 1.25,
                ],
            )
        })
        .collect();
    for &(line, baseline) in &lines {
        let [left, top, right, bottom] =
            text::label_bounds(baseline, line, style.text_height_mm, 1.);
        paper_label_mask(
            paper,
            [[left, top], [right, top], [right, bottom], [left, bottom]],
        );
    }
    for (line, baseline) in lines {
        paper.text(baseline, line, style.text_height_mm);
    }
    Ok(())
}
