//! Lightweight construction guides, never B-rep or a boolean result. The
//! accepted request still goes to the normal kernel only when Apply runs.

use limo_cad_core::PlaneBasis;
use limo_cad_solid::{ExtrudeExtent, ExtrudeOperation, ExtrudeRequest, PathRefDto, ProfileRefDto};

use crate::native_viewport::{ViewportArrow, ViewportLineLayer, ViewportModel, ViewportPreview};

type ProfileSource = Result<(PlaneBasis, Vec<Vec<[f64; 3]>>), String>;

const MAX_SEGMENTS: usize = 100_000;

pub(super) fn profile_hover(
    profile: &ProfileRefDto,
    model: &ViewportModel,
) -> Result<ViewportPreview, String> {
    let points = extrude::source_triangles(
        &ExtrudeRequest {
            sketch_name: profile.sketch_name.clone(),
            profile_indices: vec![profile.profile_index],
            source_face: None,
            operation: ExtrudeOperation::NewBody,
            extent: ExtrudeExtent::Distance { distance: 1. },
            taper_angle_deg: 0.,
            flip: false,
            target_body_ids: vec![],
        },
        model,
    )?;
    let positions: Vec<f32> = points.into_iter().flatten().map(|v| v as f32).collect();
    let sketch = model
        .document
        .profile_catalog
        .iter()
        .find(|s| s.sketch_name == profile.sketch_name)
        .ok_or("The hovered sketch is unavailable")?;
    let mut segments = Vec::new();
    for region in sketch.profiles.iter().filter(|p| {
        p.index == profile.profile_index || p.parent_index == Some(profile.profile_index)
    }) {
        for (a, b) in region
            .points
            .iter()
            .zip(region.points.iter().cycle().skip(1))
            .take(region.points.len())
        {
            segments.extend(sketch.basis.to_3d([a.x, a.y]).map(|v| v as f32));
            segments.extend(sketch.basis.to_3d([b.x, b.y]).map(|v| v as f32));
            if segments.len() / 6 > MAX_SEGMENTS {
                return Err("The hovered profile is too large to preview".into());
            }
        }
    }
    if positions
        .iter()
        .chain(segments.iter())
        .any(|v| !v.is_finite())
    {
        return Err("The hovered profile exceeds renderer range".into());
    }
    Ok(ViewportPreview {
        triangles: vec![crate::native_viewport::ViewportTriangleLayer {
            color: [1., 0.65, 0.2, 0.25],
            positions: positions.into(),
            xray: true,
            ..Default::default()
        }],
        lines: vec![crate::native_viewport::ViewportLineLayer {
            color: [1., 0.66, 0.25, 1.],
            width: 2.,
            segments: segments.into(),
            ..Default::default()
        }],
        ..Default::default()
    })
}

pub(super) fn path_hover(
    path: &PathRefDto,
    model: &ViewportModel,
) -> Result<ViewportPreview, String> {
    let sketch = model
        .document
        .finished_sketches
        .iter()
        .find(|s| s.name == path.sketch_name)
        .ok_or("The hovered path is unavailable")?;
    let mut segments = Vec::new();
    for entity in sketch
        .entities
        .iter()
        .filter(|e| path.entity_ids.contains(&e.id().0))
    {
        for pair in curve_points(entity).windows(2) {
            for point in pair {
                segments.extend(sketch.basis.to_3d([point.x, point.y]).map(|v| v as f32));
            }
            if segments.len() / 6 > MAX_SEGMENTS {
                return Err("The hovered path is too large to preview".into());
            }
        }
    }
    if segments.iter().any(|v| !v.is_finite()) {
        return Err("The hovered path exceeds renderer range".into());
    }
    Ok(ViewportPreview {
        lines: vec![crate::native_viewport::ViewportLineLayer {
            color: [1., 0.66, 0.25, 1.],
            width: 3.,
            segments: segments.into(),
            ..Default::default()
        }],
        ..Default::default()
    })
}

pub(super) fn references(
    form: &crate::native_forms::SolidForm,
    model: &crate::native_forms::FormModel<'_>,
    viewport: &ViewportModel,
) -> Result<ViewportPreview, String> {
    let mut segments = Vec::new();
    let mut triangles = Vec::new();
    let mut plane_lines = Vec::new();
    let mut profile_segments = Vec::new();
    for body in form.selected_bodies() {
        triangles.push(body_fill(model.scene, *body, [1., 0.65, 0.25, 0.25])?);
        if triangles
            .iter()
            .map(|t| t.positions.len() / 9)
            .sum::<usize>()
            > MAX_SEGMENTS
        {
            return Err("Selected bodies are too large to highlight together".into());
        }
    }
    let axis_center = form.plane_axis().and_then(|(body, edge)| {
        let edge = model
            .scene
            .bodies
            .iter()
            .find(|b| b.id == body)?
            .edges
            .iter()
            .find(|e| e.id == edge)?;
        let a = edge.points.first()?;
        let b = edge.points.last()?;
        Some([(a.x + b.x) * 0.5, (a.y + b.y) * 0.5, (a.z + b.z) * 0.5])
    });
    let centered = |mut basis: PlaneBasis| {
        if let Some(point) = axis_center {
            basis.origin = basis.to_3d(basis.to_2d(point));
        }
        basis
    };
    let mut arrows = Vec::new();
    if let Some((hole, basis, depth)) = form.hole_guide(model) {
        hole_guides(
            &hole,
            basis,
            depth,
            &mut segments,
            &mut triangles,
            &mut arrows,
        )?;
    }
    if let Some((cylinder, range)) = form.thread_guide(model) {
        use bevy::math::DVec3;
        let origin = DVec3::new(cylinder.origin.x, cylinder.origin.y, cylinder.origin.z);
        let axis = DVec3::new(cylinder.axis.x, cylinder.axis.y, cylinder.axis.z).normalize();
        let reference = DVec3::new(
            cylinder.reference.x,
            cylinder.reference.y,
            cylinder.reference.z,
        );
        let u = (reference - axis * reference.dot(axis))
            .try_normalize()
            .ok_or("The cylindrical reference direction is invalid")?;
        let v = axis.cross(u);
        let radius = cylinder.radius + (cylinder.radius * 0.0006).max(0.0004);
        let point = |z: f64, i: usize| {
            let angle = std::f64::consts::TAU * i as f64 / 48.;
            (origin + axis * z + radius * (u * angle.cos() + v * angle.sin()))
                .as_vec3()
                .to_array()
        };
        let mut positions = Vec::with_capacity(48 * 18);
        for i in 0..48 {
            let [a, b, c, d] = [
                point(range[0], i),
                point(range[0], i + 1),
                point(range[1], i + 1),
                point(range[1], i),
            ];
            positions.extend([a, b, c, a, c, d].into_iter().flatten());
        }
        let start = (origin + axis * range[0]).as_vec3().to_array();
        let end = (origin + axis * range[1]).as_vec3().to_array();
        if positions
            .iter()
            .chain(start.iter())
            .chain(end.iter())
            .any(|v| !v.is_finite())
        {
            return Err("The thread preview exceeds the renderer's range".into());
        }
        triangles.push(crate::native_viewport::ViewportTriangleLayer {
            color: [0.45, 0.72, 1., 0.14],
            positions: positions.into(),
            xray: true,
            ..Default::default()
        });
        arrows.push(ViewportArrow {
            start,
            end,
            color: [0.45, 0.72, 1., 1.],
            width: 2.,
            xray: true,
        });
    }
    if let Some((basis, distance)) = form.plane_offset(model) {
        arrows.push(ViewportArrow {
            start: basis.origin.map(|v| v as f32),
            end: std::array::from_fn(|i| (basis.origin[i] + basis.normal[i] * distance) as f32),
            color: [1., 0.72, 0.15, 1.],
            width: 3.,
            xray: true,
        });
    }
    for basis in form.plane_guides(model) {
        plane_quad(
            centered(basis),
            [0.35, 0.65, 1., 0.12],
            &mut triangles,
            &mut plane_lines,
        )?;
    }
    if let Some(basis) = form.plane_preview(model)? {
        plane_quad(
            centered(basis),
            [1., 0.72, 0.15, 0.26],
            &mut triangles,
            &mut plane_lines,
        )?;
    }
    for (body, edge) in form.plane_axis().into_iter().chain(form.pattern_edges()) {
        if let Some(edge) = model
            .scene
            .bodies
            .iter()
            .find(|b| b.id == body)
            .and_then(|b| b.edges.iter().find(|e| e.id == edge))
        {
            for pair in edge.points.windows(2) {
                for point in pair {
                    segments.extend([point.x as f32, point.y as f32, point.z as f32]);
                }
            }
        }
    }
    for (field, color) in [
        (
            crate::native_forms::SolidField::TargetBody,
            [1., 0.65, 0.25, 0.30],
        ),
        (
            crate::native_forms::SolidField::ToolBodies,
            [0.25, 0.7, 1., 0.30],
        ),
    ] {
        for id in form.combine_bodies(field) {
            triangles.push(body_fill(model.scene, id, color)?);
            if triangles
                .iter()
                .map(|t| t.positions.len() / 9)
                .sum::<usize>()
                > MAX_SEGMENTS
            {
                return Err("Selected bodies are too large to highlight together".into());
            }
        }
    }
    if let Some(face) = form.planar_source() {
        triangles.push(face_fill(
            model.scene,
            face.body_id,
            &[face.face_id],
            [1., 0.80, 0.25, 0.16],
        )?);
    }
    if let Some(face) = form.thread_face() {
        triangles.push(face_fill(
            model.scene,
            face.body_id,
            &[face.face_id],
            [1., 0.80, 0.25, 0.35],
        )?);
    }
    if let Some(face) = form.hole_support() {
        triangles.push(face_fill(
            model.scene,
            face.body_id,
            &[face.face_id],
            [1., 0.80, 0.25, 0.18],
        )?);
    }
    if let Some((body, faces)) = form.selected_faces() {
        triangles.push(face_fill(model.scene, body, faces, [1., 0.80, 0.25, 0.35])?);
    }
    if let Some((id, edges)) = form.selected_edges() {
        if let Some(body) = model.scene.bodies.iter().find(|b| b.id == id) {
            for edge in body.edges.iter().filter(|edge| edges.contains(&edge.id)) {
                for pair in edge.points.windows(2) {
                    if segments.len() / 6 >= MAX_SEGMENTS {
                        return Err("Selected edges are too large to preview".into());
                    }
                    for point in pair {
                        segments.extend([point.x as f32, point.y as f32, point.z as f32]);
                    }
                }
            }
        }
    }
    for selected in form.selected_profiles() {
        if let Some(sketch) = model
            .profiles
            .iter()
            .find(|s| s.sketch_name == selected.sketch_name)
        {
            for profile in sketch.profiles.iter().filter(|p| {
                p.index == selected.profile_index || p.parent_index == Some(selected.profile_index)
            }) {
                for (a, b) in profile
                    .points
                    .iter()
                    .zip(profile.points.iter().cycle().skip(1))
                    .take(profile.points.len())
                {
                    if (segments.len() + profile_segments.len()) / 6 >= MAX_SEGMENTS {
                        return Err("Selected profile is too large to preview".into());
                    }
                    profile_segments.extend(sketch.basis.to_3d([a.x, a.y]).map(|v| v as f32));
                    profile_segments.extend(sketch.basis.to_3d([b.x, b.y]).map(|v| v as f32));
                }
            }
        }
    }
    let mut selected_by_sketch = std::collections::BTreeMap::<String, Vec<u32>>::new();
    for profile in form.selected_profiles() {
        selected_by_sketch
            .entry(profile.sketch_name)
            .or_default()
            .push(profile.profile_index);
    }
    for (sketch_name, profile_indices) in selected_by_sketch {
        let positions = extrude::source_triangles(
            &ExtrudeRequest {
                sketch_name,
                profile_indices,
                source_face: None,
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 1. },
                taper_angle_deg: 0.,
                flip: false,
                target_body_ids: vec![],
            },
            viewport,
        )?;
        triangles.push(crate::native_viewport::ViewportTriangleLayer {
            color: [1., 0.80, 0.25, 0.16],
            positions: positions
                .into_iter()
                .flatten()
                .map(|v| v as f32)
                .collect::<Vec<_>>()
                .into(),
            xray: true,
            ..Default::default()
        });
        if triangles
            .iter()
            .map(|t| t.positions.len() / 9)
            .sum::<usize>()
            > MAX_SEGMENTS
        {
            return Err("Selected regions are too large to highlight together".into());
        }
    }
    for path in form.selected_paths() {
        if let Some(sketch) = viewport
            .document
            .finished_sketches
            .iter()
            .find(|s| s.name == path.sketch_name)
        {
            for entity in sketch
                .entities
                .iter()
                .filter(|e| path.entity_ids.contains(&e.id().0))
            {
                for pair in curve_points(entity).windows(2) {
                    if segments.len() / 6 >= MAX_SEGMENTS {
                        return Err("Selected paths are too large to preview".into());
                    }
                    for point in pair {
                        segments.extend(sketch.basis.to_3d([point.x, point.y]).map(|v| v as f32));
                    }
                }
            }
        }
    }
    if let Some(axis) = form.revolution_axis(model)? {
        segments.extend(axis.into_iter().flatten().map(|v| v as f32));
    }
    if segments
        .iter()
        .chain(profile_segments.iter())
        .any(|v| !v.is_finite())
    {
        return Err("Feature reference exceeds the renderer's range".into());
    }
    if !profile_segments.is_empty() {
        plane_lines.push(ViewportLineLayer {
            color: [1., 0.80, 0.25, 1.],
            width: 1.5,
            segments: profile_segments.into(),
            ..Default::default()
        });
    }
    plane_lines.push(ViewportLineLayer {
        color: if form.selected_edges().is_some() {
            [1., 0.88, 0.35, 1.]
        } else {
            [0.45, 0.72, 1., 1.]
        },
        width: 3.,
        segments: segments.into(),
        ..Default::default()
    });
    Ok(ViewportPreview {
        triangles,
        arrows,
        lines: plane_lines,
        ..Default::default()
    })
}

pub(super) fn body_fill(
    scene: &limo_cad_solid::SolidSceneDto,
    id: limo_cad_core::BodyId,
    color: [f32; 4],
) -> Result<crate::native_viewport::ViewportTriangleLayer, String> {
    let body = scene
        .bodies
        .iter()
        .find(|b| b.id == id)
        .ok_or("Selected body no longer exists")?;
    face_fill(
        scene,
        id,
        &body.faces.iter().map(|f| f.id).collect::<Vec<_>>(),
        color,
    )
}

pub(super) fn face_fill(
    scene: &limo_cad_solid::SolidSceneDto,
    body: limo_cad_core::BodyId,
    faces: &[limo_cad_core::FaceId],
    color: [f32; 4],
) -> Result<crate::native_viewport::ViewportTriangleLayer, String> {
    let body = scene
        .bodies
        .iter()
        .find(|b| b.id == body)
        .ok_or("Selected body no longer exists")?;
    let selected: std::collections::HashSet<_> = faces.iter().collect();
    let mut positions = Vec::new();
    for face in body.faces.iter().filter(|f| selected.contains(&f.id)) {
        let start = face.first_index as usize;
        let end = start
            .checked_add(face.index_count as usize)
            .ok_or("Face tessellation is too large")?;
        let indices = body
            .mesh
            .indices
            .get(start..end)
            .ok_or("Face tessellation is incomplete")?;
        if indices.len() % 3 != 0 || positions.len() / 9 + indices.len() / 3 > MAX_SEGMENTS {
            return Err("Selected faces are too large to preview".into());
        }
        for &index in indices {
            let start = (index as usize)
                .checked_mul(3)
                .ok_or("Face vertex exceeds renderer range")?;
            let point = body
                .mesh
                .positions
                .get(start..start + 3)
                .ok_or("Face vertex is missing")?;
            if point.iter().any(|v| !v.is_finite()) {
                return Err("Face vertex exceeds renderer range".into());
            }
            positions.extend_from_slice(point);
        }
    }
    Ok(crate::native_viewport::ViewportTriangleLayer {
        positions: positions.into(),
        color,
        ..Default::default()
    })
}

fn plane_quad(
    basis: PlaneBasis,
    color: [f32; 4],
    triangles: &mut Vec<crate::native_viewport::ViewportTriangleLayer>,
    lines: &mut Vec<ViewportLineLayer>,
) -> Result<(), String> {
    let points = [[-40., -40.], [40., -40.], [40., 40.], [-40., 40.]]
        .map(|p| basis.to_3d(p).map(|v| v as f32));
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Construction preview exceeds renderer range".into());
    }
    let mut segments = Vec::with_capacity(24);
    for i in 0..4 {
        segments.extend(points[i]);
        segments.extend(points[(i + 1) % 4]);
    }
    lines.push(ViewportLineLayer {
        segments: segments.into(),
        color: [color[0], color[1], color[2], 1.],
        width: 2.,
        ..Default::default()
    });
    triangles.push(crate::native_viewport::ViewportTriangleLayer {
        positions: [0, 1, 2, 0, 2, 3]
            .into_iter()
            .flat_map(|i| points[i])
            .collect::<Vec<_>>()
            .into(),
        color,
        ..Default::default()
    });
    Ok(())
}

fn curve_points(entity: &limo_cad_sketch::EntityDto) -> Vec<limo_cad_sketch::Vec2> {
    use limo_cad_sketch::{EntityDto, Vec2};
    let (center, radius, start, sweep) = match entity {
        EntityDto::Line { start, end, .. } => return vec![*start, *end],
        EntityDto::Spline { tessellation, .. } => return tessellation.clone(),
        EntityDto::Circle { center, radius, .. } => (*center, *radius, 0., std::f64::consts::TAU),
        EntityDto::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        } => (
            *center,
            *radius,
            *start_angle,
            (end_angle - start_angle).rem_euclid(std::f64::consts::TAU),
        ),
        _ => return vec![],
    };
    let steps = ((sweep * 30.).ceil() as usize).clamp(12, 192);
    (0..=steps)
        .map(|i| {
            let a = start + sweep * i as f64 / steps as f64;
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect()
}

mod extrude;
pub(super) use extrude::{build, handle};

fn source(request: &ExtrudeRequest, model: &ViewportModel) -> ProfileSource {
    if let Some(source) = request.source_face {
        let body = model
            .document
            .scene
            .bodies
            .iter()
            .find(|body| body.id == source.body_id)
            .ok_or("Source body is unavailable")?;
        let face = body
            .faces
            .iter()
            .find(|face| face.id == source.face_id)
            .ok_or("Source face is unavailable")?;
        let basis = face.plane.ok_or("Source face is not planar")?;
        let mut boundaries = Vec::new();
        for key in &face.edge_keys {
            let edge = body
                .edges
                .iter()
                .find(|edge| &edge.key == key)
                .ok_or("Source face boundary is unavailable")?;
            boundaries.push(edge.points.iter().map(|p| [p.x, p.y, p.z]).collect());
        }
        Ok((basis, boundaries))
    } else {
        let catalog = model
            .document
            .profile_catalog
            .iter()
            .find(|catalog| catalog.sketch_name == request.sketch_name)
            .ok_or("Source sketch is unavailable")?;
        let mut boundaries = Vec::new();
        for profile in &catalog.profiles {
            let selected = request.profile_indices.contains(&profile.index)
                || (profile.nesting_depth % 2 == 1
                    && profile
                        .parent_index
                        .is_some_and(|parent| request.profile_indices.contains(&parent)));
            if !selected {
                continue;
            }
            let mut points: Vec<_> = profile
                .points
                .iter()
                .map(|point| catalog.basis.to_3d([point.x, point.y]))
                .collect();
            if points.len() >= 2 && points.first() != points.last() {
                points.push(points[0]);
            }
            boundaries.push(points);
        }
        Ok((catalog.basis, boundaries))
    }
}

fn offsets(
    request: &ExtrudeRequest,
    model: &ViewportModel,
    basis: PlaneBasis,
) -> Result<(f64, f64, f64), String> {
    let sign = if request.flip { -1. } else { 1. };
    let offsets = match request.extent {
        ExtrudeExtent::Distance { distance } => (0., distance * sign, distance * sign),
        ExtrudeExtent::TwoSides {
            distance,
            second_distance,
        } => (-second_distance * sign, distance * sign, distance * sign),
        ExtrudeExtent::Symmetric { distance } => {
            (-distance * 0.5, distance * 0.5, distance * 0.5 * sign)
        }
        ExtrudeExtent::ToFace { face_id } => {
            let stop = model
                .document
                .scene
                .bodies
                .iter()
                .flat_map(|body| &body.faces)
                .find(|face| face.id == face_id)
                .and_then(|face| face.plane)
                .ok_or("Stop face is unavailable")?;
            let denominator = dot(basis.normal, stop.normal);
            if denominator.abs() < 1. - 1e-6 {
                return Err("To Face currently requires a parallel planar face".into());
            }
            let distance = dot(sub(stop.origin, basis.origin), stop.normal) / denominator;
            if distance.abs() <= 1e-6 {
                return Err("Stop face has no extrusion distance at the source origin".into());
            }
            (0., distance, distance)
        }
        ExtrudeExtent::ThroughAll => {
            let mut minimum = f64::INFINITY;
            let mut maximum = f64::NEG_INFINITY;
            for body in &model.document.scene.bodies {
                if !request.target_body_ids.is_empty()
                    && !request.target_body_ids.contains(&body.id)
                {
                    continue;
                }
                for point in body.mesh.positions.as_chunks::<3>().0 {
                    let distance = dot(
                        sub(
                            [point[0] as f64, point[1] as f64, point[2] as f64],
                            basis.origin,
                        ),
                        basis.normal,
                    );
                    minimum = minimum.min(distance);
                    maximum = maximum.max(distance);
                }
            }
            if !minimum.is_finite() || !maximum.is_finite() {
                return Err("Through All needs target geometry for a bounded preview".into());
            }
            let padding = 2_f64.max((maximum - minimum) * 0.08);
            (
                minimum - padding,
                maximum + padding,
                if request.flip {
                    minimum - padding
                } else {
                    maximum + padding
                },
            )
        }
    };
    if ![offsets.0, offsets.1, offsets.2]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err("Extrude preview distance is not finite".into());
    }
    Ok(offsets)
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f64; 3], scale: f64) -> [f64; 3] {
    a.map(|v| v * scale)
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn hole_guides(
    hole: &limo_cad_solid::HoleRequest,
    basis: PlaneBasis,
    depth: f64,
    segments: &mut Vec<f32>,
    triangles: &mut Vec<crate::native_viewport::ViewportTriangleLayer>,
    arrows: &mut Vec<ViewportArrow>,
) -> Result<(), String> {
    use limo_cad_solid::{HoleBottomStyle, HoleExtent, HoleStyle};
    let radius = hole.diameter * 0.5;
    let mut levels = vec![(0., radius), (depth, radius)];
    match hole.style {
        HoleStyle::Simple => (),
        HoleStyle::Counterbore => {
            levels = vec![
                (0., hole.counterbore_diameter * 0.5),
                (hole.counterbore_depth, hole.counterbore_diameter * 0.5),
                (hole.counterbore_depth, radius),
                (depth, radius),
            ]
        }
        HoleStyle::Countersink => {
            let outer = hole.countersink_diameter * 0.5;
            let sink_depth =
                (outer - radius) / (hole.countersink_angle_deg.to_radians() * 0.5).tan();
            levels = vec![(0., outer), (sink_depth, radius), (depth, radius)];
        }
    }
    if matches!(hole.extent, HoleExtent::Distance { .. })
        && hole.bottom_style == HoleBottomStyle::DrillPoint
    {
        levels.push((
            depth + radius / (hole.drill_point_angle_deg.to_radians() * 0.5).tan(),
            0.,
        ));
    }
    if hole
        .positions
        .len()
        .checked_mul(levels.len() * 48)
        .is_none_or(|n| n > MAX_SEGMENTS / 2)
    {
        return Err("Too many hole guides to preview together".into());
    }
    let sign = if hole.flip { 1. } else { -1. };
    let mut fill = Vec::new();
    for position in &hole.positions {
        let center = basis.to_3d([position.position.x, position.position.y]);
        let point = |z: f64, r: f64, i: usize| -> [f32; 3] {
            let a = std::f64::consts::TAU * i as f64 / 48.;
            std::array::from_fn(|j| {
                (center[j]
                    + sign * z * basis.normal[j]
                    + r * (a.cos() * basis.u[j] + a.sin() * basis.v[j])) as f32
            })
        };
        for &(z, r) in &levels {
            for i in 0..48 {
                segments.extend(point(z, r, i));
                segments.extend(point(z, r, i + 1));
            }
        }
        for level in levels.windows(2) {
            let (za, ra) = level[0];
            let (zb, rb) = level[1];
            for i in 0..48 {
                let [a, b, c, d] = [
                    point(za, ra, i),
                    point(za, ra, i + 1),
                    point(zb, rb, i + 1),
                    point(zb, rb, i),
                ];
                fill.extend([a, b, c, a, c, d].into_iter().flatten());
                if i % 12 == 0 {
                    segments.extend(a);
                    segments.extend(d);
                }
            }
        }
        arrows.push(ViewportArrow {
            start: center.map(|v| v as f32),
            end: point(depth, 0., 0),
            color: [0.45, 0.72, 1., 1.],
            width: 2.,
            xray: true,
        });
    }
    if fill.iter().chain(segments.iter()).any(|v| !v.is_finite()) {
        return Err("The hole preview exceeds the renderer's range".into());
    }
    triangles.push(crate::native_viewport::ViewportTriangleLayer {
        color: [0.45, 0.72, 1., 0.14],
        positions: fill.into(),
        xray: true,
        ..Default::default()
    });
    Ok(())
}
