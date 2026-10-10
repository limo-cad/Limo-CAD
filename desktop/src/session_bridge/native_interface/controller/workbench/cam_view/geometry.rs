//! Presentation of the shared planner's motion, never a second CAM planner.
use crate::native_viewport::{
    ViewportArrow, ViewportCamTool, ViewportLineLayer, ViewportPreview, ViewportTriangleLayer,
};
use limo_cad_cam::{
    CamArcPlane, CamCommandDto, CamDocumentDto, CamProgramDto, CamResolvedStockDto, CamSetupDto,
    Point3Dto, WorkCoordinateSystemDto,
};

const RAPID: [f32; 4] = [0.94, 0.67, 0.29, 0.8];
const CUT: [f32; 4] = [0.34, 0.84, 0.64, 0.95];
const MAX_PATH_SEGMENTS: usize = 65_000;

pub(super) fn model_point(point: Point3Dto, wcs: WorkCoordinateSystemDto) -> [f32; 3] {
    let origin = [wcs.origin.x, wcs.origin.y, wcs.origin.z];
    std::array::from_fn(|i| {
        (origin[i] + point.x * wcs.x_axis[i] + point.y * wcs.y_axis[i] + point.z * wcs.z_axis[i])
            as f32
    })
}

pub(super) fn stock(setup: &CamSetupDto, fill: bool) -> ViewportPreview {
    let mut preview = ViewportPreview::default();
    let wcs = setup.wcs;
    let length = 4_f64.max(
        (setup.stock.max.x - setup.stock.min.x).min(setup.stock.max.y - setup.stock.min.y) * 0.12,
    );
    for (axis, color) in [
        (wcs.x_axis, [0.93, 0.42, 0.35, 1.]),
        (wcs.y_axis, [0.34, 0.84, 0.64, 1.]),
        (wcs.z_axis, [0.4, 0.73, 0.94, 1.]),
    ] {
        let start = [
            wcs.origin.x as f32,
            wcs.origin.y as f32,
            wcs.origin.z as f32,
        ];
        preview.arrows.push(ViewportArrow {
            start,
            end: std::array::from_fn(|i| start[i] + (axis[i] * length) as f32),
            color,
            width: 2.,
            xray: true,
        });
    }
    let mut ring = Vec::new();
    match setup.resolved_stock {
        CamResolvedStockDto::Cylinder { center, radius } => {
            for i in 0..64 {
                let a = i as f64 * std::f64::consts::TAU / 64.;
                ring.push([center.x + radius * a.cos(), center.y + radius * a.sin()]);
            }
        }
        CamResolvedStockDto::Hex {
            center,
            across_flats,
        } => {
            for i in 0..6 {
                let a = std::f64::consts::PI / 6. + i as f64 * std::f64::consts::TAU / 6.;
                let radius = across_flats / 3_f64.sqrt();
                ring.push([center.x + radius * a.cos(), center.y + radius * a.sin()]);
            }
        }
        _ => ring.extend([
            [setup.stock.min.x, setup.stock.min.y],
            [setup.stock.max.x, setup.stock.min.y],
            [setup.stock.max.x, setup.stock.max.y],
            [setup.stock.min.x, setup.stock.max.y],
        ]),
    }
    let mut edges = Vec::new();
    let mut triangles = Vec::new();
    let point = |xy: [f64; 2], z| model_point(Point3Dto::new(xy[0], xy[1], z), wcs);
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        let a = point(ring[i], setup.stock.min.z);
        let b = point(ring[j], setup.stock.min.z);
        let c = point(ring[j], setup.stock.max.z);
        let d = point(ring[i], setup.stock.max.z);
        edges.extend(a);
        edges.extend(b);
        edges.extend(d);
        edges.extend(c);
        edges.extend(a);
        edges.extend(d);
        triangles.extend([a, b, c, a, c, d].into_iter().flatten());
        if i > 0 && i + 1 < ring.len() {
            triangles.extend(
                [
                    point(ring[0], setup.stock.min.z),
                    b,
                    a,
                    point(ring[0], setup.stock.max.z),
                    d,
                    c,
                ]
                .into_iter()
                .flatten(),
            );
        }
    }
    preview.lines.push(ViewportLineLayer {
        color: [0.62, 0.68, 0.75, 0.5],
        width: 1.,
        segments: edges.into(),
        ..Default::default()
    });
    if fill && !matches!(setup.resolved_stock, CamResolvedStockDto::ModelBody { .. }) {
        preview.triangles.push(ViewportTriangleLayer {
            color: [0.62, 0.68, 0.75, 0.16],
            positions: triangles.into(),
            ..Default::default()
        });
    }
    preview
}

fn components(point: Point3Dto, plane: CamArcPlane) -> [f64; 3] {
    match plane {
        CamArcPlane::Xy => [point.x, point.y, point.z],
        CamArcPlane::Xz => [point.z, point.x, point.y],
        CamArcPlane::Yz => [point.y, point.z, point.x],
    }
}
fn from_components([u, v, w]: [f64; 3], plane: CamArcPlane) -> Point3Dto {
    match plane {
        CamArcPlane::Xy => Point3Dto::new(u, v, w),
        CamArcPlane::Xz => Point3Dto::new(v, w, u),
        CamArcPlane::Yz => Point3Dto::new(w, u, v),
    }
}

/// Same arc-plane convention and bounded chord density as the shared viewport.
pub(super) fn arc_points(
    from: Point3Dto,
    to: Point3Dto,
    center: Point3Dto,
    plane: CamArcPlane,
    clockwise: bool,
) -> Vec<Point3Dto> {
    let [su, sv, sw] = components(from, plane);
    let [eu, ev, ew] = components(to, plane);
    let [cu, cv, _] = components(center, plane);
    let start = (sv - cv).atan2(su - cu);
    let mut sweep = (ev - cv).atan2(eu - cu) - start;
    if clockwise {
        while sweep >= 0. {
            sweep -= std::f64::consts::TAU;
        }
    } else {
        while sweep <= 0. {
            sweep += std::f64::consts::TAU;
        }
    }
    let radius = (su - cu).hypot(sv - cv);
    let count = ((sweep.abs() * radius / 1.5).ceil() as usize).clamp(8, 96);
    (1..=count)
        .map(|i| {
            if i == count {
                return to;
            }
            let t = i as f64 / count as f64;
            let a = start + sweep * t;
            from_components(
                [
                    cu + radius * a.cos(),
                    cv + radius * a.sin(),
                    sw + (ew - sw) * t,
                ],
                plane,
            )
        })
        .collect()
}

pub(super) fn paths(
    document: &CamDocumentDto,
    setup: &CamSetupDto,
    program: &CamProgramDto,
    selected: Option<u64>,
) -> Result<(Vec<ViewportLineLayer>, Option<ViewportCamTool>), String> {
    let mut rapid = Vec::new();
    let mut cutting = Vec::new();
    let mut position = None;
    let mut section = None;
    let mut tool = None;
    let mut first_tool = None;
    let mut first_position = None;
    let mut count = 0;
    for command in &program.commands {
        match command {
            CamCommandDto::SectionStart {
                operation_id,
                tool_id,
                ..
            } => {
                section = Some(*operation_id);
                tool = Some(*tool_id);
                position = None;
            }
            CamCommandDto::SectionEnd => {
                section = None;
                position = None;
            }
            CamCommandDto::SetPosition { to } => {
                position = Some(*to);
            }
            CamCommandDto::Rapid { to }
            | CamCommandDto::Linear { to, .. }
            | CamCommandDto::Circular { to, .. } => {
                let visible = section.is_some() && (selected.is_none() || selected == section);
                if visible {
                    if let Some(from) = position {
                        let points = match command {
                            CamCommandDto::Circular {
                                center,
                                plane,
                                clockwise,
                                ..
                            } => arc_points(from, *to, *center, *plane, *clockwise),
                            _ => vec![*to],
                        };
                        count += points.len();
                        if count > MAX_PATH_SEGMENTS {
                            return Err("Toolpath display exceeds 65,000 segments; select one operation to inspect it".into());
                        }
                        let target = if matches!(command, CamCommandDto::Rapid { .. }) {
                            &mut rapid
                        } else {
                            &mut cutting
                        };
                        let mut prior = from;
                        for p in points {
                            target.extend(model_point(prior, setup.wcs));
                            target.extend(model_point(p, setup.wcs));
                            prior = p;
                        }
                    }
                    if first_tool.is_none() {
                        first_tool = tool;
                        first_position = Some(*to);
                    }
                }
                position = Some(*to);
            }
            _ => {}
        }
    }
    let tool = first_tool.zip(first_position).and_then(|(id, tip)| {
        document.tool(id).map(|tool| ViewportCamTool {
            tip: model_point(tip, setup.wcs),
            axis: setup.wcs.z_axis.map(|v| v as f32),
            geometry: tool.into(),
        })
    });
    Ok((
        [(RAPID, rapid), (CUT, cutting)]
            .into_iter()
            .filter(|(_, segments)| !segments.is_empty())
            .map(|(color, segments)| ViewportLineLayer {
                color,
                width: 2.,
                segments: segments.into(),
                ..Default::default()
            })
            .collect(),
        tool,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_coordinates_preserve_rotated_translated_wcs() {
        let wcs = WorkCoordinateSystemDto {
            origin: Point3Dto::new(10., 20., 30.),
            x_axis: [0., 1., 0.],
            y_axis: [-1., 0., 0.],
            z_axis: [0., 0., 1.],
        };
        assert_eq!(model_point(Point3Dto::new(2., 3., 4.), wcs), [7., 22., 34.]);
    }
    #[test]
    fn display_arcs_keep_exact_endpoints_handedness_and_helical_axes() {
        for plane in [CamArcPlane::Xy, CamArcPlane::Xz, CamArcPlane::Yz] {
            let from = from_components([5., 0., 2.], plane);
            let to = from_components([0., 5., 7.], plane);
            let points = arc_points(from, to, Point3Dto::new(0., 0., 0.), plane, false);
            assert_eq!(points.last(), Some(&to));
            let first = components(points[0], plane);
            assert!(first[0] > 0. && first[1] > 0. && first[2] > 2. && first[2] < 7.);
            for p in points {
                let [u, v, _] = components(p, plane);
                assert!((u.hypot(v) - 5.).abs() < 1e-10);
            }
            let clockwise = arc_points(from, to, Point3Dto::new(0., 0., 0.), plane, true);
            assert!(components(clockwise[0], plane)[1] < 0.);
        }
    }
}
