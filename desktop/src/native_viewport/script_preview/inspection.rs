//! Native labels for immutable inspection results; no engine calls.
pub(crate) use limo_cad_native_engine::teaching::Inspection;

pub(crate) fn annotations(
    inspection: &Inspection,
    camera: crate::native_viewport::ViewportCamera,
    size: [f32; 2],
    raster: [f32; 2],
) -> Result<crate::native_editor::annotations::teaching::Annotations, String> {
    use crate::native_editor::annotations::teaching::{Annotations, Label};
    let mut out = Annotations::default();
    let mut segments = Vec::new();
    if let Some(m) = &inspection.measurement {
        if let (Some(points), Some(distance)) = (m.report.closest_points, m.report.distance) {
            segments.push((points, distance));
        }
    }
    if let Some(section) = &inspection.section {
        if let Some(probe) = section.request.probe_mm {
            use limo_cad_occt::section_review::SectionPlane;
            let point = |u| match section.request.plane {
                SectionPlane::Xy => [u, probe, section.request.offset_mm],
                SectionPlane::Xz => [u, section.request.offset_mm, probe],
                SectionPlane::Yz => [section.request.offset_mm, u, probe],
            };
            for span in &section.probe_spans {
                segments.push(([point(span.start_mm), point(span.end_mm)], span.length_mm));
            }
        }
    }
    let project = |p| {
        crate::native_viewport::project_world_point(camera, raster, p)
            .map(|p| p.map(|p| [p[0] * size[0] / raster[0], p[1] * size[1] / raster[1]]))
    };
    for (points, distance) in segments {
        if let (Some(a), Some(b)) = (project(points[0])?, project(points[1])?) {
            out.lines.push([a, b]);
            let number = format!("{distance:.3}");
            let text = format!("{} mm", number.trim_end_matches('0').trim_end_matches('.'));
            let width = (text.chars().count() as f32 * 7. + 12.)
                .max(36.)
                .min(size[0]);
            out.labels.push(Label {
                text,
                center: [
                    ((a[0] + b[0]) * 0.5).clamp(width * 0.5, size[0] - width * 0.5),
                    ((a[1] + b[1]) * 0.5 - 14.).clamp(10., size[1] - 10.),
                ],
                width,
                angle: 0.,
            });
        }
    }
    Ok(out)
}
