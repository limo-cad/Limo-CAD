//! Graphical SVG/DXF presentation of the existing straight dimension records.
use super::*;
use crate::drawing_presentation::{geometry, linear, text};
use limo_cad_core::UnitSystem;

pub(super) fn point(
    reference: &DrawingTopologyAnchorRefDto,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
) -> Result<P, String> {
    if projection
        .topology_signatures
        .get(&reference.body_id.0.to_string())
        != reference.topology_signature.as_ref()
    {
        return Err("Straight dimension projection has a stale topology signature".into());
    }
    let point = paper_point(view, anchor_point(reference, projection)?, projection);
    if point.iter().any(|value| !value.is_finite()) {
        return Err("Straight dimension contains non-finite projected coordinates".into());
    }
    Ok(point)
}

fn line(
    reference: &DrawingLineRefDto,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
) -> Result<[P; 2], String> {
    let endpoint = |endpoint| {
        point(
            &DrawingTopologyAnchorRefDto {
                topology_signature: reference.topology_signature.clone(),
                occurrence_id: reference.occurrence_id,
                body_id: reference.body_id,
                edge_id: reference.edge_id,
                edge_key: reference.edge_key.clone(),
                endpoint,
                fallback_point: [0.; 3],
                circle_center: false,
            },
            view,
            projection,
        )
    };
    Ok([
        endpoint(DrawingEdgeEndpoint::Start)?,
        endpoint(DrawingEdgeEndpoint::End)?,
    ])
}

pub(super) fn filled_arrow(paper: &mut Paper, tip: P, base: P) {
    let delta = geometry::sub(base, tip);
    let side = geometry::scale(geometry::normal(delta), 0.3);
    paper.items.push(Primitive::Triangle {
        points: [tip, geometry::add(base, side), geometry::sub(base, side)],
        layer: "DIMENSION",
    });
}

pub(super) fn draw_linear(
    paper: &mut Paper,
    sheet: &DrawingSheetDto,
    geometry: geometry::Linear,
    value: String,
    presentation: &DrawingDimensionPresentationDto,
) -> Result<(), String> {
    if !geometry.value.is_finite()
        || geometry.value <= 0.
        || [
            geometry.first,
            geometry.second,
            geometry.start,
            geometry.end,
        ]
        .iter()
        .flatten()
        .any(|x| !x.is_finite())
    {
        return Err("Straight dimension geometry exceeds finite paper coordinates".into());
    }
    let layout = linear::layout(
        geometry.first,
        geometry.second,
        geometry.start,
        geometry.end,
        &value,
        &sheet.style,
        sheet.standard,
    );
    for points in layout.extensions {
        paper.line(points.to_vec(), "EXTENSION", &sheet.style.extension);
    }
    for points in layout
        .unmasked_shaft
        .into_iter()
        .take(layout.unmasked_shaft_count)
    {
        paper.line(points.to_vec(), "DIMENSION", &sheet.style.dimension);
    }
    if presentation.basic {
        paper_label_mask(
            paper,
            basic_label_corners(
                layout.text_baseline,
                &value,
                sheet.style.text_height_mm,
                Some(layout.text_angle.to_degrees()),
            ),
        );
    }
    for [tip, base] in layout.arrows {
        filled_arrow(paper, tip, base);
    }
    dimension_label(
        paper,
        layout.text_baseline,
        value,
        presentation,
        &sheet.style,
        Some(layout.text_angle.to_degrees()),
    );
    Ok(())
}

fn draw_angular(
    paper: &mut Paper,
    sheet: &DrawingSheetDto,
    geometry: geometry::Angular,
    value: String,
    presentation: &DrawingDimensionPresentationDto,
) -> Result<(), String> {
    if !geometry.value.is_finite()
        || [
            geometry.vertex,
            geometry.first,
            geometry.second,
            geometry.text,
        ]
        .iter()
        .chain(geometry.points.iter())
        .flatten()
        .any(|x| !x.is_finite())
    {
        return Err("Straight angle geometry exceeds finite paper coordinates".into());
    }
    let style = &sheet.style;
    paper.line(
        vec![geometry.vertex, geometry.first],
        "EXTENSION",
        &style.extension,
    );
    paper.line(
        vec![geometry.vertex, geometry.second],
        "EXTENSION",
        &style.extension,
    );
    let points = &geometry.points;
    for (tip, toward) in [
        (points[0], points[1]),
        (points[points.len() - 1], points[points.len() - 2]),
    ] {
        if let Some(direction) = geometry::unit(geometry::sub(toward, tip)) {
            filled_arrow(
                paper,
                tip,
                geometry::add(tip, geometry::scale(direction, style.arrow_size_mm)),
            );
        }
    }
    paper.line(geometry.points, "DIMENSION", &style.dimension);
    angular_label_mask(paper, geometry.text, &value, presentation, style);
    dimension_label(paper, geometry.text, value, presentation, style, Some(0.));
    Ok(())
}

pub(super) fn draw(
    paper: &mut Paper,
    sheet: &DrawingSheetDto,
    projections: &BTreeMap<u64, DrawingProjectionDto>,
    annotation: &DrawingAnnotationDto,
    units: UnitSystem,
) -> Result<(), String> {
    match annotation {
        DrawingAnnotationDto::LineDimension {
            view_id,
            first,
            second,
            mode,
            position,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            let first = line(first, view, projection)?;
            let second = second
                .as_ref()
                .map(|r| line(r, view, projection))
                .transpose()?;
            let geometry = geometry::line_dimension(first, second, *mode, *position, view.scale)
                .ok_or(
                    "Straight dimension is collapsed or incompatible with the current projection",
                )?;
            match geometry {
                geometry::LineDimension::Linear(geometry) => {
                    let value = dimension_text(
                        geometry.value,
                        *precision,
                        prefix,
                        suffix,
                        presentation,
                        units,
                    );
                    draw_linear(paper, sheet, geometry, value, presentation)?;
                }
                geometry::LineDimension::Angular(geometry) => {
                    let mut format = presentation.clone();
                    format.basic = false;
                    let value = text::angular(geometry.value, *precision, prefix, suffix, &format);
                    draw_angular(paper, sheet, geometry, value, presentation)?;
                }
            }
        }
        DrawingAnnotationDto::PointLineDimension {
            view_id,
            point: reference,
            line: edge,
            position,
            precision,
            prefix,
            suffix,
            presentation,
            ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            let geometry = geometry::point_line(
                point(reference, view, projection)?, line(edge, view, projection)?, *position, view.scale,
            ).ok_or("Point-line dimension has zero distance or a collapsed line in the current projection")?;
            let value = dimension_text(
                geometry.value,
                *precision,
                prefix,
                suffix,
                presentation,
                units,
            );
            draw_linear(paper, sheet, geometry, value, presentation)?;
        }
        _ => unreachable!("Only straight dimensions enter this painter"),
    }
    Ok(())
}
