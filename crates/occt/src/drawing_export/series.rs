//! Chain, baseline, continued and ordinate exports over existing saved intent.
use super::*;
use crate::drawing_presentation::{geometry, text};

pub(super) fn draw(
    sheet: &DrawingSheetDto,
    projections: &BTreeMap<u64, DrawingProjectionDto>,
    annotation: &DrawingAnnotationDto,
    units: limo_cad_core::UnitSystem,
    budget: &mut PaperGraphicsBudget,
) -> Result<Vec<Primitive>, String> {
    let mut graphics = graphics::Graphics::new(budget);
    let size = sheet_size(sheet);
    match annotation {
        DrawingAnnotationDto::ChainDimension {
            view_id,
            anchors,
            mode,
            layout,
            offset,
            spacing,
            prefix,
            suffix,
            precision,
            presentation,
            ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            graphics.budget.work(
                (anchors.len() as u64)
                    .checked_mul((projection.anchors.len() + projection.circles.len()) as u64)
                    .ok_or("Series resolution work overflow")?,
            )?;
            let text_bytes = prefix
                .len()
                .checked_add(suffix.len())
                .ok_or("Series text storage overflow")?;
            graphics.budget.work(text_bytes as u64)?;
            let text_lines = prefix
                .lines()
                .count()
                .checked_add(suffix.lines().count())
                .and_then(|n| n.checked_add(8))
                .ok_or("Series text storage overflow")?;
            graphics.budget.scratch(
                text_bytes
                    .checked_mul(8)
                    .and_then(|n| {
                        text_lines
                            .checked_mul(std::mem::size_of::<Primitive>() * 2)
                            .and_then(|lines| n.checked_add(lines))
                    })
                    .and_then(|n| n.checked_add(64 * 1024))
                    .ok_or("Series text storage overflow")?,
            )?;
            let anchors = anchors
                .iter()
                .map(|a| straight::point(a, view, projection))
                .collect::<Result<Vec<_>, _>>()?;
            for (index, second) in anchors.iter().skip(1).enumerate() {
                let baseline = *layout == DrawingChainDimensionLayout::Baseline;
                let first = anchors[if baseline { 0 } else { index }];
                let offset = offset + if baseline { index as f64 * spacing } else { 0. };
                let span = geometry::dimension_span(*mode, first, *second, offset, view.scale)
                    .ok_or("Series dimension is collapsed or exceeds finite paper coordinates")?;
                let value =
                    dimension_text(span.value, *precision, prefix, suffix, presentation, units);
                let mut paper = Paper {
                    size,
                    items: Vec::new(),
                };
                straight::draw_linear(&mut paper, sheet, span, value, presentation)?;
                admit(&mut graphics, paper, annotation.id())?;
            }
        }
        DrawingAnnotationDto::OrdinateDimension {
            view_id,
            origin,
            target,
            axis,
            offset,
            precision,
            presentation,
            ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            graphics.budget.work(
                (projection.anchors.len() as u64)
                    .checked_add(projection.circles.len() as u64)
                    .and_then(|n| n.checked_mul(2))
                    .ok_or("Ordinate resolution work overflow")?,
            )?;
            graphics.budget.scratch(64 * 1024)?;
            let mut paper = Paper {
                size,
                items: Vec::new(),
            };
            let origin = straight::point(origin, view, projection)?;
            let target = straight::point(target, view, projection)?;
            let g = geometry::ordinate(origin, target, *offset, view.scale)
                .ok_or("Ordinate dimension is collapsed or exceeds finite paper coordinates")?;
            let x = text::dimension(g.x_value, *precision, "X", "", units, presentation);
            let y = text::dimension(g.y_value, *precision, "Y", "", units, presentation);
            let value = match axis {
                DrawingOrdinateAxis::X => x,
                DrawingOrdinateAxis::Y => y,
                DrawingOrdinateAxis::Both => format!("{x}  {y}"),
            };
            let ring = geometry::arc(g.origin, 1.2, 0., std::f64::consts::TAU);
            for pair in ring.windows(2) {
                paper.items.push(Primitive::Triangle {
                    points: [g.origin, pair[0], pair[1]],
                    layer: TEXT_MASK,
                });
            }
            paper.line(
                ring,
                "DIMENSION",
                &DrawingLineStyleDto {
                    width_mm: 0.45,
                    dash_mm: vec![],
                },
            );
            paper.line(
                vec![g.target, g.elbow, g.position],
                "DIMENSION",
                &sheet.style.dimension,
            );
            if let Some(direction) = geometry::unit(geometry::sub(g.elbow, g.target)) {
                straight::filled_arrow(
                    &mut paper,
                    g.target,
                    geometry::add(
                        g.target,
                        geometry::scale(direction, sheet.style.arrow_size_mm),
                    ),
                );
            }
            let baseline = [g.position[0], g.position[1] - 0.7];
            let [left, top, right, bottom] =
                text::label_bounds(baseline, &value, sheet.style.text_height_mm, 1.);
            paper_label_mask(
                &mut paper,
                [[left, top], [right, top], [right, bottom], [left, bottom]],
            );
            paper.text(baseline, value, sheet.style.text_height_mm);
            admit(&mut graphics, paper, annotation.id())?;
        }
        _ => return Err("Expected a series or ordinate dimension".into()),
    }
    Ok(graphics.finish())
}

fn admit(graphics: &mut graphics::Graphics<'_>, paper: Paper, id: u64) -> Result<(), String> {
    for item in paper.items {
        let (points, margin) = match &item {
            Primitive::Line { points, width, .. } => (points.clone(), width * 0.5),
            Primitive::Triangle { points, .. } => (points.to_vec(), 0.),
            Primitive::Text {
                point,
                value,
                height,
                centered,
                rotation_deg,
                ..
            } => {
                let [left, top, right, bottom] =
                    text::label_bounds(*point, value, *height, if *centered { 0. } else { 1. });
                let angle = rotation_deg.to_radians();
                let points =
                    [[left, top], [right, top], [right, bottom], [left, bottom]].map(|p| {
                        let d = geometry::sub(p, *point);
                        geometry::add(
                            *point,
                            [
                                d[0] * angle.cos() - d[1] * angle.sin(),
                                d[0] * angle.sin() + d[1] * angle.cos(),
                            ],
                        )
                    });
                (points.to_vec(), 0.)
            }
        };
        if points.iter().any(|p| {
            p.iter().any(|n| !n.is_finite())
                || p[0] - margin < 0.
                || p[1] - margin < 0.
                || p[0] + margin > paper.size[0]
                || p[1] + margin > paper.size[1]
        }) {
            return Err(format!("Annotation {id} extends beyond the sheet; move its view or dimension, or choose a larger sheet before exporting"));
        }
        graphics.primitive(item)?;
    }
    Ok(())
}
