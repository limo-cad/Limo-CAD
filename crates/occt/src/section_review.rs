//! Disposable source-body sections, with bounded native sampling and no HLR.
//! Drawing sections share exact primitives; inspection creates no CAD feature.
use crate::drawing_export::{
    section_hatch, HatchPattern, PaperGraphicsBudget, PaperGraphicsLimits, PaperPrimitive,
};
use crate::{DrawingPolylineDto, DrawingProjectionDto};
use limo_cad_core::BodyId;
use limo_cad_sketch::{DrawingLineStyleDto, DrawingViewDto, DrawingViewKind};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionPlane {
    Xy,
    Xz,
    Yz,
}
impl SectionPlane {
    pub fn axis(self) -> usize {
        match self {
            Self::Xy => 2,
            Self::Xz => 1,
            Self::Yz => 0,
        }
    }
    pub fn labels(self) -> [&'static str; 3] {
        match self {
            Self::Xy => ["X", "Y", "Z"],
            Self::Xz => ["X", "Z", "Y"],
            Self::Yz => ["Y", "Z", "X"],
        }
    }
    pub fn basis(self) -> ([f64; 3], [f64; 3]) {
        match self {
            Self::Xy => ([0., 0., 1.], [0., 1., 0.]),
            Self::Xz => ([0., -1., 0.], [0., 0., 1.]),
            Self::Yz => ([1., 0., 0.], [0., 0., 1.]),
        }
    }
}
fn deflection() -> f64 {
    0.01
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionReviewRequest {
    pub body_id: BodyId,
    pub plane: SectionPlane,
    pub offset_mm: f64,
    /// Optional horizontal probe at this vertical model coordinate, in mm.
    #[serde(default)]
    pub probe_mm: Option<f64>,
    #[serde(default = "deflection")]
    pub deflection_mm: f64,
    /// Request a disposable, capped OCCT half-solid for the modeling viewport.
    #[serde(default)]
    pub include_cutaway: bool,
    /// Retain coordinates above the plane; false retains those below it.
    /// This selects only the 3D cutaway. Contours/probes describe cut material
    /// on the exact plane, excluding coplanar exterior termination faces.
    #[serde(default)]
    pub keep_positive: bool,
}
impl SectionReviewRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.body_id.0 == 0
            || !self.offset_mm.is_finite()
            || self.probe_mm.is_some_and(|p| !p.is_finite())
        {
            return Err("Choose a current body and finite section/probe coordinates".into());
        }
        if !self.deflection_mm.is_finite() || !(0.001..=0.1).contains(&self.deflection_mm) {
            return Err("Section contour sampling must be between 0.001 and 0.1 mm".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionSpan {
    pub start_mm: f64,
    pub end_mm: f64,
    pub length_mm: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionReview {
    pub request: SectionReviewRequest,
    pub outcome: SectionOutcome,
    pub bounds_mm: Option<[f64; 4]>,
    pub section: Vec<DrawingPolylineDto>,
    pub probe_spans: Vec<SectionSpan>,
    pub svg: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutaway: Option<limo_cad_solid::KernelBodyDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionOutcome {
    NoIntersection,
    /// Contact without splitting any connected source solid's volume.
    BoundaryContact,
    /// Regularized plane / 3D-interior material; exterior coplanar faces do not
    /// become hatch/probe area, even when another region cuts real material.
    MaterialSection,
}

pub(crate) struct SectionGeometry {
    pub outcome: SectionOutcome,
    pub section: Vec<DrawingPolylineDto>,
    pub cutaway: Option<limo_cad_solid::KernelBodyDto>,
}

#[derive(Debug)]
pub enum SectionReviewError {
    InvalidRequest(String),
    Kernel {
        body_id: BodyId,
        plane: SectionPlane,
        offset_mm: f64,
        source: crate::OcctError,
    },
    Presentation(String),
}
impl std::fmt::Display for SectionReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) | Self::Presentation(message) => f.write_str(message),
            Self::Kernel {
                body_id,
                plane,
                offset_mm,
                source,
            } => write!(
                f,
                "Section inspection for body {}, {plane:?} at {offset_mm} mm: {source}",
                body_id.0
            ),
        }
    }
}
impl std::error::Error for SectionReviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Kernel { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Shared native/hosted read. Source scope and error handling are identical.
pub fn inspect(
    kernel: &crate::OcctKernel,
    scene: &limo_cad_solid::SolidSceneDto,
    request: &SectionReviewRequest,
) -> Result<SectionReview, SectionReviewError> {
    request
        .validate()
        .map_err(SectionReviewError::InvalidRequest)?;
    if !scene.errors.is_empty() {
        return Err(SectionReviewError::InvalidRequest(
            "Resolve timeline errors before inspecting a section".into(),
        ));
    }
    if !scene.bodies.iter().any(|body| body.id == request.body_id) {
        return Err(SectionReviewError::InvalidRequest(
            "Section body is missing".into(),
        ));
    }
    let geometry =
        kernel
            .section_geometry(request)
            .map_err(|source| SectionReviewError::Kernel {
                body_id: request.body_id,
                plane: request.plane,
                offset_mm: request.offset_mm,
                source,
            })?;
    let projection = DrawingProjectionDto {
        topology_signatures: Default::default(),
        visible: vec![],
        hidden: vec![],
        anchors: vec![],
        circles: vec![],
        section: geometry.section,
        bounds: [0.; 4],
    };
    let mut report =
        present(request, projection, geometry.outcome).map_err(SectionReviewError::Presentation)?;
    if report.outcome == SectionOutcome::MaterialSection {
        report.cutaway = geometry.cutaway;
    }
    Ok(report)
}

/// Half-open crossings avoid double-counting contour vertices. Zero-length
/// tangent intervals are omitted; odd crossings fail rather than invent a wall.
pub fn probe_spans(lines: &[DrawingPolylineDto], v: f64) -> Result<Vec<SectionSpan>, String> {
    if !v.is_finite() {
        return Err("Probe coordinate must be finite".into());
    }
    let mut hits = Vec::new();
    for line in lines {
        if !line.points.iter().flatten().all(|v| v.is_finite()) {
            return Err("Section contains non-finite coordinates".into());
        }
        for pair in line.points.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            if (a[1] <= v && v < b[1]) || (b[1] <= v && v < a[1]) {
                let u = a[0] + (v - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                if !u.is_finite() {
                    return Err("Section probe is non-finite".into());
                }
                hits.push(u);
            }
        }
    }
    hits.sort_by(f64::total_cmp);
    if !hits.len().is_multiple_of(2) {
        return Err("Probe crosses an open or ambiguous section contour; move it slightly".into());
    }
    let spans: Vec<_> = hits
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|h| h[1] - h[0] > 1e-8)
        .map(|h| SectionSpan {
            start_mm: h[0],
            end_mm: h[1],
            length_mm: h[1] - h[0],
        })
        .collect();
    if spans.len() > 128 {
        return Err("Section probe exceeds 128 material intervals".into());
    }
    Ok(spans)
}

pub fn present(
    request: &SectionReviewRequest,
    mut projection: DrawingProjectionDto,
    outcome: SectionOutcome,
) -> Result<SectionReview, String> {
    request.validate()?;
    let count: usize = projection.section.iter().map(|l| l.points.len()).sum();
    if count > 100_000 {
        return Err("Section diagram exceeds 100,000 contour points".into());
    }
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in projection.section.iter().flat_map(|l| &l.points) {
        if !p.iter().all(|v| v.is_finite()) {
            return Err("Section contains non-finite coordinates".into());
        }
        bounds[0] = bounds[0].min(p[0]);
        bounds[1] = bounds[1].min(p[1]);
        bounds[2] = bounds[2].max(p[0]);
        bounds[3] = bounds[3].max(p[1]);
    }
    if count == 0 {
        return Ok(SectionReview {
            request: request.clone(),
            outcome,
            bounds_mm: None,
            section: vec![],
            probe_spans: vec![],
            svg: String::new(),
            cutaway: None,
        });
    }
    if bounds[2] - bounds[0] < 1e-8 || bounds[3] - bounds[1] < 1e-8 {
        return Ok(SectionReview {
            request: request.clone(),
            outcome: SectionOutcome::BoundaryContact,
            bounds_mm: Some(bounds),
            section: projection.section,
            probe_spans: vec![],
            svg: String::new(),
            cutaway: None,
        });
    }
    let spans = request
        .probe_mm
        .filter(|_| outcome == SectionOutcome::MaterialSection)
        .map(|v| probe_spans(&projection.section, v))
        .transpose()?
        .unwrap_or_default();
    projection.bounds = bounds;
    let svg = diagram(request, &projection, bounds, &spans, outcome)?;
    Ok(SectionReview {
        request: request.clone(),
        outcome,
        bounds_mm: Some(bounds),
        section: projection.section,
        probe_spans: spans,
        svg,
        cutaway: None,
    })
}

fn diagram(
    req: &SectionReviewRequest,
    projection: &DrawingProjectionDto,
    b: [f64; 4],
    spans: &[SectionSpan],
    outcome: SectionOutcome,
) -> Result<String, String> {
    let w = b[2] - b[0];
    let h = b[3] - b[1];
    let scale = (680. / w).min(400. / h);
    let left = 60. + (680. - w * scale) / 2.;
    let top = 70. + (400. - h * scale) / 2.;
    let model = |p: [f64; 2]| [left + (p[0] - b[0]) * scale, top + (b[3] - p[1]) * scale];
    let paper = |p: [f64; 2]| [left + (p[0] - b[0]) * scale, top + (p[1] - b[1]) * scale];
    let labels = req.plane.labels();
    let mut svg = String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 0 800 600"><rect width="800" height="600" fill="white"/><g font-family="sans-serif" fill="#203040">"##,
    );
    write!(
        svg,
        r#"<text x="30" y="30" font-size="18">Body {} · {} = {:.4} mm section</text>"#,
        req.body_id.0, labels[2], req.offset_mm
    )
    .unwrap();
    let (direction, up) = req.plane.basis();
    if outcome == SectionOutcome::BoundaryContact {
        svg.push_str(r#"<text x="30" y="54" font-size="13">Boundary contact; no interior material spans or retained cutaway</text>"#);
    }
    let view = DrawingViewDto {
        scope: Default::default(),
        occurrence_ids: vec![],
        id: 0,
        name: "Section inspection".into(),
        kind: DrawingViewKind::Section,
        direction,
        up,
        position: [(b[0] + b[2]) / 2., (b[1] + b[3]) / 2.],
        scale: 1.,
        body_ids: vec![req.body_id],
        show_hidden_lines: false,
        show_tangent_edges: false,
        parent_view_id: None,
        alignment: Default::default(),
        derivation: None,
    };
    let style = DrawingLineStyleDto {
        width_mm: 0.15,
        dash_mm: vec![],
    };
    let hatch = if outcome == SectionOutcome::MaterialSection {
        section_hatch(
            &view,
            projection,
            &style,
            HatchPattern {
                angle_deg: 45.,
                spacing_mm: (w.max(h) / 55.).max(req.deflection_mm * 4.),
            },
            &mut PaperGraphicsBudget::new(PaperGraphicsLimits {
                primitives: 20_000,
                points: 100_000,
                retained_bytes: 8 * 1024 * 1024,
                scratch_bytes: 8 * 1024 * 1024,
                work: 10_000_000,
            }),
        )?
    } else {
        vec![]
    };
    svg.push_str(r##"<g fill="none" stroke="#b6c4ce" stroke-width="0.7">"##);
    for primitive in hatch {
        if let PaperPrimitive::Line { points, .. } = primitive {
            polyline(&mut svg, &points, &paper);
        }
    }
    svg.push_str(r##"</g><g fill="none" stroke="#243b4b" stroke-width="1.4">"##);
    for line in &projection.section {
        polyline(&mut svg, &line.points, &model);
    }
    svg.push_str("</g>");
    if let Some(v) = req
        .probe_mm
        .filter(|_| outcome == SectionOutcome::MaterialSection)
    {
        if (b[1]..=b[3]).contains(&v) {
            let a = model([b[0], v]);
            let z = model([b[2], v]);
            write!(svg,r##"<path d="M {} {} L {} {}" fill="none" stroke="#ba661d" stroke-dasharray="4 4"/>"##,a[0],a[1],z[0],z[1]).unwrap();
            for s in spans {
                let a = model([s.start_mm, v]);
                let z = model([s.end_mm, v]);
                write!(svg,r##"<path d="M {} {} L {} {}" stroke="#ba661d" stroke-width="3"/><text x="{}" y="{}" text-anchor="middle" font-size="12">{:.3} mm</text>"##,a[0],a[1],z[0],z[1],(a[0]+z[0])/2.,a[1]-7.,s.length_mm).unwrap();
            }
        }
    }
    write!(svg,r#"<text x="400" y="504" text-anchor="middle" font-size="14">{} coordinate (mm): {:.3} to {:.3}</text><text x="30" y="535" font-size="13">{} coordinate (mm): {:.3} to {:.3} · section extent {:.3} × {:.3} mm</text><text x="30" y="561" font-size="12">Source-body geometry · contour sampling {:.3} mm · probe spans use sampled contours</text><text x="30" y="584" font-size="12">Geometric inspection; no structural results or printer toolpaths.</text></g></svg>"#,labels[0],b[0],b[2],labels[1],b[1],b[3],w,h,req.deflection_mm).unwrap();
    Ok(svg)
}
fn polyline(svg: &mut String, points: &[[f64; 2]], map: &impl Fn([f64; 2]) -> [f64; 2]) {
    svg.push_str("<polyline points=\"");
    for p in points {
        let q = map(*p);
        write!(svg, "{:.4},{:.4} ", q[0], q[1]).unwrap();
    }
    svg.push_str("\"/>");
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square(a: f64, b: f64) -> DrawingPolylineDto {
        DrawingPolylineDto {
            points: vec![[a, a], [b, a], [b, b], [a, b], [a, a]],
        }
    }
    #[test]
    fn probe_excludes_voids_and_preserves_disconnected_material() {
        let spans = probe_spans(&[square(0., 10.), square(3., 7.), square(12., 14.)], 5.).unwrap();
        assert_eq!(
            spans,
            vec![
                SectionSpan {
                    start_mm: 0.,
                    end_mm: 3.,
                    length_mm: 3.
                },
                SectionSpan {
                    start_mm: 7.,
                    end_mm: 10.,
                    length_mm: 3.
                }
            ]
        );
        assert_eq!(
            probe_spans(&[square(0., 10.), square(12., 14.)], 13.).unwrap()[0].length_mm,
            2.
        );
        assert!(probe_spans(&[square(0., 10.)], 10.).unwrap().is_empty());
        assert_eq!(
            probe_spans(&[square(0., 10.)], 0.).unwrap()[0].length_mm,
            10.
        );
    }
    #[test]
    fn probe_rejects_open_contours_and_nonfinite_coordinates() {
        assert!(probe_spans(
            &[DrawingPolylineDto {
                points: vec![[0., 0.], [0., 10.]]
            }],
            5.
        )
        .is_err());
        assert!(probe_spans(&[], f64::NAN).is_err());
    }
    #[test]
    fn axial_sections_have_the_expected_model_coordinate_basis() {
        for plane in [SectionPlane::Xy, SectionPlane::Xz, SectionPlane::Yz] {
            let (d, u) = plane.basis();
            let right = [
                u[1] * d[2] - u[2] * d[1],
                u[2] * d[0] - u[0] * d[2],
                u[0] * d[1] - u[1] * d[0],
            ];
            assert_eq!(
                right,
                if plane == SectionPlane::Yz {
                    [0., 1., 0.]
                } else {
                    [1., 0., 0.]
                }
            );
        }
    }
}
