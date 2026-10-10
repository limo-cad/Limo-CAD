//! The same translucent tool volume and selected region used by noBS.
//! Earcut preserves concave regions and holes; this never changes the B-rep.
use super::*;
use crate::native_viewport::ViewportTriangleLayer;
use std::sync::Arc;

const BLUE: [f32; 4] = [0.45, 0.72, 1., 1.];

pub(crate) fn handle(
    request: &ExtrudeRequest,
    model: &ViewportModel,
) -> Result<(PlaneBasis, [f64; 3], f64), String> {
    let (basis, _) = source(request, model)?;
    Ok((
        basis,
        area_center(&source_triangles(request, model)?)?,
        offsets(request, model, basis)?.2,
    ))
}

fn area_center(fill: &[[f64; 3]]) -> Result<[f64; 3], String> {
    let mut moment = [0.; 3];
    let mut weight = 0.;
    // Triangles describe the filled area, so holes do not add spurious weight.
    for triangle in fill.as_chunks::<3>().0.iter() {
        let a = sub(triangle[1], triangle[0]);
        let b = sub(triangle[2], triangle[0]);
        let cross = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let area = dot(cross, cross).sqrt() * 0.5;
        moment = add(
            moment,
            scale(add(add(triangle[0], triangle[1]), triangle[2]), area / 3.),
        );
        weight += area;
    }
    if weight <= 1e-9 {
        return Err("No source area is available for the extrusion handle".into());
    }
    Ok(scale(moment, 1. / weight))
}

pub(super) fn source_triangles(
    request: &ExtrudeRequest,
    model: &ViewportModel,
) -> Result<Vec<[f64; 3]>, String> {
    if let Some(source) = request.source_face {
        let body = model
            .document
            .scene
            .bodies
            .iter()
            .find(|b| b.id == source.body_id)
            .ok_or("Source body is unavailable")?;
        let face = body
            .faces
            .iter()
            .find(|f| f.id == source.face_id)
            .ok_or("Source face is unavailable")?;
        let start = face.first_index as usize;
        let end = start
            .checked_add(face.index_count as usize)
            .ok_or("Source face mesh is too large")?;
        let indices = body
            .mesh
            .indices
            .get(start..end)
            .ok_or("Source face mesh is incomplete")?;
        if indices.len() % 3 != 0 || indices.len() / 3 > MAX_SEGMENTS {
            return Err("The source face is too large to preview".into());
        }
        return indices
            .iter()
            .map(|i| {
                let start = (*i as usize)
                    .checked_mul(3)
                    .ok_or("Source face vertex exceeds renderer range")?;
                let point = body
                    .mesh
                    .positions
                    .get(start..start + 3)
                    .ok_or("Source face vertex is unavailable")?;
                if point.iter().any(|v| !v.is_finite()) {
                    return Err("Source face vertex exceeds renderer range".into());
                }
                Ok([point[0] as f64, point[1] as f64, point[2] as f64])
            })
            .collect();
    }
    let catalog = model
        .document
        .profile_catalog
        .iter()
        .find(|c| c.sketch_name == request.sketch_name)
        .ok_or("Source sketch is unavailable")?;
    let mut result = Vec::new();
    for outer in catalog
        .profiles
        .iter()
        .filter(|p| p.nesting_depth % 2 == 0 && request.profile_indices.contains(&p.index))
    {
        let mut flat = Vec::new();
        let mut holes = Vec::new();
        for (index, loop_) in std::iter::once(outer)
            .chain(
                catalog
                    .profiles
                    .iter()
                    .filter(|p| p.nesting_depth % 2 == 1 && p.parent_index == Some(outer.index)),
            )
            .enumerate()
        {
            let mut points = Vec::<[f64; 2]>::new();
            for p in &loop_.points {
                if !p.x.is_finite() || !p.y.is_finite() {
                    return Err("The profile contains an invalid point".into());
                }
                let next = [p.x, p.y];
                if points
                    .last()
                    .is_none_or(|last| (last[0] - p.x).abs() > 1e-9 || (last[1] - p.y).abs() > 1e-9)
                {
                    points.push(next);
                }
            }
            if points.len() > 1
                && points.first().zip(points.last()).is_some_and(|(a, b)| {
                    (a[0] - b[0]).abs() <= 1e-9 && (a[1] - b[1]).abs() <= 1e-9
                })
            {
                points.pop();
            }
            if points.len() < 3 {
                return Err("The source profile has no closed region".into());
            }
            if index > 0 {
                holes.push(flat.len() / 2);
            }
            flat.extend(points.into_iter().flatten());
            if flat.len() / 2 > MAX_SEGMENTS {
                return Err("The profile is too large to preview".into());
            }
        }
        let indices = earcutr::earcut(&flat, &holes, 2)
            .map_err(|e| format!("The source region could not be filled: {e}"))?;
        if indices.is_empty() {
            return Err("The source profile has no area".into());
        }
        result.extend(
            indices
                .into_iter()
                .map(|i| catalog.basis.to_3d([flat[i * 2], flat[i * 2 + 1]])),
        );
        if result.len() / 3 > MAX_SEGMENTS {
            return Err("The selected regions are too large to preview".into());
        }
    }
    Ok(result)
}

pub(crate) fn build(
    request: &ExtrudeRequest,
    model: &ViewportModel,
) -> Result<ViewportPreview, String> {
    if request.taper_angle_deg.abs() > 1e-9 {
        return Err("Taper will be calculated by the kernel on Apply; an untapered preview would be misleading".into());
    }
    let (basis, boundaries) = source(request, model)?;
    let (start, end, direction) = offsets(request, model, basis)?;
    let offset = |p, distance| add(p, scale(basis.normal, distance));
    let source_fill = source_triangles(request, model)?;
    let center = area_center(&source_fill)?;
    let mut fill = Vec::new();
    for triangle in source_fill.as_chunks::<3>().0.iter() {
        fill.extend(triangle.iter().map(|p| offset(*p, start)));
        fill.extend(triangle.iter().rev().map(|p| offset(*p, end)));
    }
    let mut outline = Vec::new();
    for boundary in boundaries {
        for pair in boundary.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            let [a0, b0, a1, b1] = [
                offset(a, start),
                offset(b, start),
                offset(a, end),
                offset(b, end),
            ];
            fill.extend([a0, b0, b1, a0, b1, a1]);
            outline.extend([a, b]);
            if fill.len() / 3 > MAX_SEGMENTS {
                return Err("The profile exceeds the interactive preview limit".into());
            }
        }
    }
    let floats = |points: Vec<[f64; 3]>| -> Result<Arc<Vec<f32>>, String> {
        let values: Vec<_> = points.into_iter().flatten().map(|v| v as f32).collect();
        if values.iter().any(|v| !v.is_finite()) {
            return Err("Extrude preview exceeds the renderer range".into());
        }
        Ok(values.into())
    };
    let color = match request.operation {
        ExtrudeOperation::Cut => [1., 0.42, 0.37, 0.20],
        ExtrudeOperation::Join => [0.31, 0.79, 0.55, 0.20],
        ExtrudeOperation::Intersect => [0.69, 0.55, 1., 0.20],
        _ => [BLUE[0], BLUE[1], BLUE[2], 0.20],
    };
    Ok(ViewportPreview {
        triangles: vec![
            ViewportTriangleLayer {
                color,
                positions: floats(fill)?,
                xray: true,
                ..Default::default()
            },
            ViewportTriangleLayer {
                color: [1., 0.80, 0.25, 0.16],
                positions: floats(source_fill)?,
                xray: true,
                ..Default::default()
            },
        ],
        lines: vec![ViewportLineLayer {
            color: [1., 0.80, 0.25, 1.],
            width: 1.5,
            segments: floats(outline)?,
            ..Default::default()
        }],
        arrows: vec![ViewportArrow {
            start: center.map(|v| v as f32),
            end: offset(center, direction).map(|v| v as f32),
            color: BLUE,
            width: 2.,
            xray: true,
        }],
        ..Default::default()
    })
}
