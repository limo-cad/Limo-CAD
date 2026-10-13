//! Center markings over current projected circular references.
use super::*;
use crate::drawing_presentation::{centers, geometry};

pub(super) fn caption_baseline(
    baseline: f64,
    sheet: &DrawingSheetDto,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
) -> Result<f64, String> {
    let mut bottom: Option<f64> = None;
    for annotation in &sheet.annotations {
        let extent = match annotation {
            DrawingAnnotationDto::CenterMark {
                view_id,
                feature,
                extension,
                ..
            } if *view_id == view.id => {
                let (center, radius) = circle(feature, view, projection)?;
                let lines =
                    centers::mark(center, radius, *extension).ok_or("Invalid center extent")?;
                let stroke = lines
                    .iter()
                    .flatten()
                    .map(|p| p[1])
                    .fold(f64::NEG_INFINITY, f64::max)
                    + sheet.style.center.width_mm * 0.5;
                stroke.max(center[1] + 0.66)
            }
            DrawingAnnotationDto::CenterLine {
                view_id,
                first,
                second,
                extension,
                ..
            } if *view_id == view.id => {
                let (a, ar) = circle(first, view, projection)?;
                let (b, br) = circle(second, view, projection)?;
                let line =
                    centers::line(a, ar, b, br, *extension).ok_or("Invalid centerline extent")?;
                (line[0][1].max(line[1][1]) + sheet.style.center.width_mm * 0.5)
                    .max(a[1].max(b[1]) + 0.66)
            }
            _ => continue,
        };
        bottom = Some(bottom.map_or(extent, |prior| prior.max(extent)));
    }
    Ok(centers::caption_baseline(
        baseline,
        sheet.style.small_text_height_mm,
        bottom,
    ))
}

pub(super) fn circle(
    reference: &DrawingCircularRefDto,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
) -> Result<(P, f64), String> {
    if projection
        .topology_signatures
        .get(&reference.body_id.0.to_string())
        != reference.topology_signature.as_ref()
    {
        return Err("Circular annotation projection has a stale topology signature".into());
    }
    let matches = |circle: &&crate::DrawingProjectedCircleDto| {
        circle.occurrence_id == reference.occurrence_id
            && circle.body_id == reference.body_id
            && circle.edge_key == reference.edge_key
    };
    let resolved = projection
        .circles
        .iter()
        .filter(matches)
        .find(|circle| circle.edge_id == reference.edge_id)
        .or_else(|| projection.circles.iter().find(matches))
        .ok_or("Circular annotation reference is missing or no longer circular in this view")?;
    if !reference.closed || !resolved.closed {
        return Err("Circular annotations require closed circular edges".into());
    }
    let center = paper_point(view, resolved.center, projection);
    let radius = resolved.radius * view.scale;
    if !radius.is_finite() || radius <= 0. || center.iter().any(|x| !x.is_finite()) {
        return Err("Circular annotation contains invalid projected geometry".into());
    }
    Ok((center, radius))
}

fn ring(paper: &mut Paper, center: P) {
    paper.line(
        geometry::arc(center, 0.48, 0., std::f64::consts::TAU),
        "CENTER_MARK",
        &DrawingLineStyleDto {
            width_mm: 0.36,
            dash_mm: vec![],
        },
    );
    let inner = geometry::arc(center, 0.3, 0., std::f64::consts::TAU);
    for pair in inner.windows(2) {
        paper.items.push(Primitive::Triangle {
            points: [center, pair[0], pair[1]],
            layer: TEXT_MASK,
        });
    }
}

pub(super) fn draw(
    paper: &mut Paper,
    sheet: &DrawingSheetDto,
    projections: &BTreeMap<u64, DrawingProjectionDto>,
    annotation: &DrawingAnnotationDto,
) -> Result<(), String> {
    match annotation {
        DrawingAnnotationDto::CenterMark {
            view_id,
            feature,
            extension,
            ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            let (center, radius) = circle(feature, view, projection)?;
            let segments = centers::mark(center, radius, *extension)
                .ok_or("Center mark exceeds finite paper coordinates")?;
            for segment in segments {
                paper.line(segment.to_vec(), "CENTER", &sheet.style.center);
            }
            ring(paper, center);
        }
        DrawingAnnotationDto::CenterLine {
            view_id,
            first,
            second,
            extension,
            ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            let (a, first_radius) = circle(first, view, projection)?;
            let (b, second_radius) = circle(second, view, projection)?;
            let segment = centers::line(a, first_radius, b, second_radius, *extension)
                .ok_or("Centerline circles coincide or exceed finite paper coordinates")?;
            paper.line(segment.to_vec(), "CENTER", &sheet.style.center);
            ring(paper, a);
            ring(paper, b);
        }
        _ => unreachable!("Only center marks and paired circles are dispatched here"),
    }
    Ok(())
}
