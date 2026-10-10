use super::graphics::Graphics;
use super::*;

/// The source marker for one derived view. The projection lookup borrows the
/// caller's existing cache; child storage order never determines its parent.
/// Native callers may initially request only Section/RemovedSection markers.
pub fn derived_source_graphics<'a>(
    child: &DrawingViewDto,
    sheet: &DrawingSheetDto,
    projection: impl Fn(u64) -> Option<&'a DrawingProjectionDto>,
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    budget: &mut PaperGraphicsBudget,
) -> Result<Vec<PaperPrimitive>, String> {
    let Some(derivation) = &child.derivation else {
        return Ok(Vec::new());
    };
    let parent_id = match derivation {
        DrawingViewDerivationDto::Section { parent_view_id, .. }
        | DrawingViewDerivationDto::RemovedSection { parent_view_id, .. }
        | DrawingViewDerivationDto::Detail { parent_view_id, .. }
        | DrawingViewDerivationDto::Auxiliary { parent_view_id, .. }
        | DrawingViewDerivationDto::Broken { parent_view_id, .. } => *parent_view_id,
    };
    let count = sheet.views.len() as u64;
    budget.work(
        count
            .checked_mul(count)
            .ok_or("Derived view lookup work overflow")?,
    )?;
    let parent = sheet
        .views
        .iter()
        .find(|v| v.id == parent_id)
        .ok_or("Dimension view missing")?;
    let projection = projection(parent_id).ok_or("Dimension projection missing")?;
    budget.work(
        (projection.anchors.len() as u64)
            .checked_mul(2)
            .ok_or("Source anchor work overflow")?,
    )?;
    let request = projection_request(parent, &sheet.views, scene, assembly)?;
    let direction = norm(request.direction)?;
    let right = norm(cross(request.up, direction))?;
    let up = norm(cross(direction, right))?;
    let source = |reference: &DrawingTopologyAnchorRefDto| -> Result<P, String> {
        if !projection.anchors.iter().any(|anchor| {
            anchor.occurrence_id == reference.occurrence_id
                && anchor.body_id == reference.body_id
                && anchor.edge_id == reference.edge_id
                && anchor.edge_key == reference.edge_key
        }) {
            return Err("Derived source reference is missing from its parent projection".into());
        }
        let point = model_anchor(reference, scene, assembly)?;
        Ok(paper_point(
            parent,
            [dot(point, right), dot(point, up)],
            projection,
        ))
    };
    let mut paper = Graphics::new(budget);
    match derivation {
        DrawingViewDerivationDto::Section {
            first,
            second,
            label,
            ..
        }
        | DrawingViewDerivationDto::RemovedSection {
            first,
            second,
            label,
            ..
        } => {
            let [a, b] =
                section_source_extent(source(first)?, source(second)?, parent, projection)?;
            let u = source_direction(a, b)?;
            let normal = [-u[1], u[0]];
            paper.line(&[a, b], "CUTTING_PLANE", &sheet.style.cutting_plane)?;
            for point in [a, b] {
                source_arrow(
                    &mut paper,
                    point,
                    [point[0] + normal[0] * 5., point[1] + normal[1] * 5.],
                    2.4,
                    "CUTTING_PLANE",
                )?;
            }
            paper.budget.work(label.len() as u64)?;
            let short_label = label.split_whitespace().last().unwrap_or(label);
            for (point, sign) in [(a, -1.), (b, 1.)] {
                paper.label(
                    [point[0] + u[0] * sign * 4., point[1] + u[1] * sign * 4.],
                    short_label,
                    sheet.style.text_height_mm,
                    true,
                )?;
            }
        }
        DrawingViewDerivationDto::Detail {
            center,
            radius,
            label,
            ..
        } => {
            let center = source(center)?;
            let radius = radius * parent.scale;
            let points: [P; 129] = std::array::from_fn(|i| {
                let a = std::f64::consts::TAU * i as f64 / 128.;
                [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
            });
            paper.line(&points, "PHANTOM", &sheet.style.phantom)?;
            paper.text(
                [center[0] + radius + 3., center[1] - radius - 1.],
                label,
                sheet.style.text_height_mm,
            )?;
        }
        DrawingViewDerivationDto::Auxiliary {
            reference,
            flipped,
            label,
            ..
        } => {
            let anchor = |endpoint| DrawingTopologyAnchorRefDto {
                topology_signature: reference.topology_signature.clone(),
                occurrence_id: reference.occurrence_id,
                body_id: reference.body_id,
                edge_id: reference.edge_id,
                edge_key: reference.edge_key.clone(),
                endpoint,
                fallback_point: [0.; 3],
                circle_center: false,
            };
            let a = source(&anchor(DrawingEdgeEndpoint::Start))?;
            let b = source(&anchor(DrawingEdgeEndpoint::End))?;
            let u = source_direction(a, b)?;
            let normal = [-u[1], u[0]];
            let center = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let sign = if *flipped { -1. } else { 1. };
            let tip = [
                center[0] + normal[0] * sign * 8.,
                center[1] + normal[1] * sign * 8.,
            ];
            paper.line(&[a, b], "PHANTOM", &sheet.style.phantom)?;
            paper.line(
                &[center, tip],
                "AUXILIARY",
                &DrawingLineStyleDto {
                    width_mm: 0.48,
                    dash_mm: vec![],
                },
            )?;
            source_arrow(&mut paper, tip, center, 2.2, "AUXILIARY")?;
            paper.label(
                [tip[0] + normal[0] * 3., tip[1] + normal[1] * 3.],
                label,
                sheet.style.text_height_mm,
                true,
            )?;
        }
        DrawingViewDerivationDto::Broken { axis, .. } => {
            let k = if *axis == DrawingBreakAxis::Horizontal {
                0
            } else {
                1
            };
            let extent = (projection.bounds[3 - k] - projection.bounds[1 - k]) * parent.scale * 0.5;
            let along = parent.position[k];
            let across = parent.position[1 - k];
            let points = [
                [along, across - extent],
                [along, across - 4.],
                [along - 2., across - 2.],
                [along + 2., across],
                [along - 2., across + 2.],
                [along, across + 4.],
                [along, across + extent],
            ]
            .map(|p| if k == 0 { p } else { [p[1], p[0]] });
            paper.line(&points, "BREAK", &sheet.style.break_line)?;
        }
    }
    Ok(paper.finish())
}

fn source_arrow(
    paper: &mut Graphics<'_>,
    tip: P,
    toward: P,
    size: f64,
    layer: &'static str,
) -> Result<(), String> {
    if let Ok(u) = source_direction(tip, toward) {
        let base = [tip[0] + u[0] * size, tip[1] + u[1] * size];
        let width = size * 0.38;
        paper.triangle(
            [
                tip,
                [base[0] - u[1] * width, base[1] + u[0] * width],
                [base[0] + u[1] * width, base[1] - u[0] * width],
            ],
            layer,
        )?;
    }
    Ok(())
}
