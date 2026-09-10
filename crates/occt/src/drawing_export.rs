//! Deterministic engineering-sheet export. Both formats use the same paper
//! primitives and current exact projection; saved fallback coordinates never
//! substitute for lost topology. Files remain exports of editable drawing DTOs.
use crate::{DrawingProjectionDto, DrawingProjectionRequest, DrawingSectionPlaneDto};
use nbcad_sketch::*;
use nbcad_solid::SolidSceneDto;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingExportFormat {
    Svg,
    Dxf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawingExportRequest {
    pub sheet_id: u64,
    pub format: DrawingExportFormat,
}

type P = [f64; 2];
#[derive(Debug)]
enum Primitive {
    Line {
        points: Vec<P>,
        layer: &'static str,
        width: f64,
        dash: Vec<f64>,
    },
    Text {
        point: P,
        value: String,
        height: f64,
    },
}
struct Paper {
    size: P,
    items: Vec<Primitive>,
}
impl Paper {
    fn line(&mut self, points: Vec<P>, layer: &'static str, style: &DrawingLineStyleDto) {
        self.items.push(Primitive::Line {
            points,
            layer,
            width: style.width_mm,
            dash: style.dash_mm.clone(),
        });
    }
    fn text(&mut self, point: P, value: impl Into<String>, height: f64) {
        for (i, line) in value.into().lines().enumerate() {
            self.items.push(Primitive::Text {
                point: [point[0], point[1] + i as f64 * height * 1.4],
                value: line.into(),
                height,
            });
        }
    }
}

/// The caller owns the kernel/session. Supplying its projection closure avoids
/// another model load, filesystem side effects, or separate rendering process.
pub fn export_sheet(
    document: &DrawingDocumentDto,
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    request: &DrawingExportRequest,
    mut project: impl FnMut(&DrawingProjectionRequest) -> Result<DrawingProjectionDto, String>,
) -> Result<String, String> {
    document.validate()?;
    if !scene.errors.is_empty() {
        return Err("Resolve timeline errors before exporting drawings.".into());
    }
    let sheet = document
        .sheets
        .iter()
        .find(|s| s.id == request.sheet_id)
        .ok_or("Drawing sheet does not exist")?;
    let mut paper = Paper {
        size: sheet_size(sheet),
        items: Vec::new(),
    };
    let [w, h] = paper.size;
    paper.line(
        vec![
            [10., 10.],
            [w - 10., 10.],
            [w - 10., h - 10.],
            [10., h - 10.],
            [10., 10.],
        ],
        "BORDER",
        &sheet.style.visible,
    );
    let title = &sheet.title_block;
    paper.line(
        vec![[10., h - 30.], [w - 10., h - 30.]],
        "BORDER",
        &sheet.style.visible,
    );
    paper.text(
        [14., h - 24.],
        format!(
            "{}   {}   Rev {}",
            if title.title.is_empty() {
                &sheet.name
            } else {
                &title.title
            },
            title.drawing_number,
            title.revision
        ),
        sheet.style.text_height_mm,
    );
    paper.text(
        [14., h - 18.],
        format!(
            "Dimensions: mm   {:?} / {:?}   Material: {}   Finish: {}",
            sheet.standard, sheet.projection_method, title.material, title.finish
        ),
        sheet.style.small_text_height_mm,
    );
    paper.text(
        [14., h - 12.],
        tolerance_note(&sheet.tolerance_note),
        sheet.style.small_text_height_mm,
    );
    let mut projections = BTreeMap::new();
    for view in &sheet.views {
        let req = projection_request(view, &sheet.views, scene, assembly)?;
        let projection = project(&req)?;
        if projection.bounds.iter().any(|v| !v.is_finite()) {
            return Err("Projection contains non-finite bounds".into());
        }
        for (lines, layer, style) in [
            (&projection.visible, "VISIBLE", &sheet.style.visible),
            (&projection.hidden, "HIDDEN", &sheet.style.hidden),
            (&projection.section, "SECTION", &sheet.style.cutting_plane),
        ] {
            if layer == "HIDDEN" && !view.show_hidden_lines {
                continue;
            }
            if matches!(
                view.derivation,
                Some(DrawingViewDerivationDto::RemovedSection { .. })
            ) && layer != "SECTION"
            {
                continue;
            }
            for line in lines {
                let points = line
                    .points
                    .iter()
                    .map(|p| paper_point(view, *p, &projection))
                    .collect::<Vec<_>>();
                if points.iter().flatten().any(|v| !v.is_finite()) {
                    return Err("Projection contains non-finite linework".into());
                }
                for points in clip_view_polyline(view, &projection, &points)? {
                    paper.line(points, layer, style);
                }
            }
        }
        if let Some(
            DrawingViewDerivationDto::Section {
                hatch_angle_deg,
                hatch_spacing_mm,
                ..
            }
            | DrawingViewDerivationDto::RemovedSection {
                hatch_angle_deg,
                hatch_spacing_mm,
                ..
            },
        ) = &view.derivation
        {
            hatch_section(
                &mut paper,
                view,
                &projection,
                *hatch_angle_deg,
                *hatch_spacing_mm,
                &sheet.style.hatch,
            )?;
        }
        if let Some(DrawingViewDerivationDto::Detail { center, radius, .. }) = &view.derivation {
            let c = paper_point(view, anchor_point(center, &projection)?, &projection);
            paper.line(
                circle_polyline(c, radius * view.scale),
                "PHANTOM",
                &sheet.style.phantom,
            );
        }
        if let Some(DrawingViewDerivationDto::Broken { axis, gap_mm, .. }) = &view.derivation {
            let k = match axis {
                DrawingBreakAxis::Horizontal => 0,
                DrawingBreakAxis::Vertical => 1,
            };
            let extent = (projection.bounds[3 - k] - projection.bounds[1 - k]) * view.scale * 0.5;
            for sign in [-1., 1.] {
                let along = view.position[k] + sign * gap_mm.max(3.) * 0.5;
                let across = view.position[1 - k];
                let points = vec![
                    [along, across - extent],
                    [along, across - 4.],
                    [along - 2., across - 2.],
                    [along + 2., across],
                    [along - 2., across + 2.],
                    [along, across + 4.],
                    [along, across + extent],
                ];
                paper.line(
                    points
                        .into_iter()
                        .map(|p| if k == 0 { p } else { [p[1], p[0]] })
                        .collect(),
                    "BREAK",
                    &sheet.style.break_line,
                );
            }
        }
        let label_y = view.position[1]
            + (projection.bounds[3] - projection.bounds[1]) * 0.5 * view.scale
            + 6.;
        paper.text(
            [
                view.position[0] - (projection.bounds[2] - projection.bounds[0]) * 0.5 * view.scale,
                label_y,
            ],
            format!("{}  (scale {})", view.name, view.scale),
            sheet.style.small_text_height_mm,
        );
        projections.insert(view.id, projection);
    }
    for annotation in &sheet.annotations {
        draw_annotation(&mut paper, sheet, &projections, annotation)?;
    }
    if !sheet.bom.is_empty() {
        let origin = sheet.bom_table_position.unwrap_or([14., 18.]);
        paper.text(
            origin,
            "ITEM   PART / DESCRIPTION   QTY   MATERIAL / PROCESS",
            sheet.style.small_text_height_mm,
        );
        for (i, item) in sheet.bom.iter().enumerate() {
            paper.text(
                [origin[0], origin[1] + (i + 1) as f64 * 5.],
                format!(
                    "{}   {} — {}   {}   {} / {}",
                    item.item_number,
                    item.part_number,
                    item.description,
                    item.quantity,
                    item.material,
                    item.finish
                ),
                sheet.style.small_text_height_mm,
            );
        }
    }
    match request.format {
        DrawingExportFormat::Svg => Ok(svg(&paper, &sheet.style.font_family)),
        DrawingExportFormat::Dxf => Ok(dxf(&paper)),
    }
}

fn sheet_size(s: &DrawingSheetDto) -> P {
    let (a, b) = match s.format {
        DrawingSheetFormat::A0 => (841., 1189.),
        DrawingSheetFormat::A1 => (594., 841.),
        DrawingSheetFormat::A2 => (420., 594.),
        DrawingSheetFormat::A3 => (297., 420.),
        DrawingSheetFormat::A4 => (210., 297.),
        DrawingSheetFormat::Letter => (215.9, 279.4),
        DrawingSheetFormat::AnsiB => (279.4, 431.8),
        DrawingSheetFormat::AnsiC => (431.8, 558.8),
        DrawingSheetFormat::AnsiD => (558.8, 863.6),
        DrawingSheetFormat::AnsiE => (863.6, 1117.6),
    };
    match s.orientation {
        DrawingSheetOrientation::Landscape => [b, a],
        DrawingSheetOrientation::Portrait => [a, b],
    }
}
fn tolerance_note(n: &DrawingToleranceNoteDto) -> String {
    match n.preset {
        DrawingTolerancePreset::None => n.custom.clone(),
        DrawingTolerancePreset::Custom => n.custom.clone(),
        DrawingTolerancePreset::Iso2768Fine => "General tolerances ISO 2768-f".into(),
        DrawingTolerancePreset::Iso2768Medium => "General tolerances ISO 2768-m".into(),
        DrawingTolerancePreset::Iso2768Coarse => "General tolerances ISO 2768-c".into(),
        DrawingTolerancePreset::Iso2768VeryCoarse => "General tolerances ISO 2768-v".into(),
        DrawingTolerancePreset::AnsiDecimal => {
            "General decimal tolerances: see specified drawing notes".into()
        }
    }
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> Result<[f64; 3], String> {
    let l = dot(a, a).sqrt();
    if l < 1e-10 || !l.is_finite() {
        return Err("Degenerate derived drawing basis".into());
    }
    Ok(a.map(|x| x / l))
}
fn model_anchor(
    a: &DrawingTopologyAnchorRefDto,
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
) -> Result<[f64; 3], String> {
    crate::resolve_drawing_anchor(scene, assembly, a).map_err(|e| e.to_string())
}
/// Resolve view intent from current topology, without accepting stale fallback
/// coordinates. Section orientation follows the same parent/cut-line basis as
/// the interactive drawing editor.
pub fn projection_request(
    view: &DrawingViewDto,
    views: &[DrawingViewDto],
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
) -> Result<DrawingProjectionRequest, String> {
    fn resolve(
        v: &DrawingViewDto,
        views: &[DrawingViewDto],
        scene: &SolidSceneDto,
        assembly: &AssemblyDocumentDto,
        path: &mut Vec<u64>,
    ) -> Result<([f64; 3], [f64; 3], Option<DrawingSectionPlaneDto>), String> {
        if path.contains(&v.id) {
            return Err("Drawing view dependency cycle".into());
        }
        path.push(v.id);
        let result = match &v.derivation {
            None => Ok((v.direction, v.up, None)),
            Some(DrawingViewDerivationDto::Section {
                parent_view_id,
                first,
                second,
                ..
            })
            | Some(DrawingViewDerivationDto::RemovedSection {
                parent_view_id,
                first,
                second,
                ..
            }) => {
                let parent = views
                    .iter()
                    .find(|p| p.id == *parent_view_id)
                    .ok_or("Derived view parent is missing")?;
                let (pd, _, _) = resolve(parent, views, scene, assembly, path)?;
                let a = model_anchor(first, scene, assembly)?;
                let b = model_anchor(second, scene, assembly)?;
                let edge = norm(std::array::from_fn(|i| b[i] - a[i]))?;
                let mut direction = norm(cross(edge, norm(pd)?))?;
                if dot(direction, v.direction) < 0. {
                    direction = direction.map(|x| -x);
                }
                let mut up = norm(cross(direction, edge))?;
                if dot(up, v.up) < 0. {
                    up = up.map(|x| -x);
                }
                let depth = match &v.derivation {
                    Some(DrawingViewDerivationDto::Section { depth, .. }) => *depth,
                    _ => None,
                };
                Ok((
                    direction,
                    up,
                    Some(DrawingSectionPlaneDto {
                        point: a,
                        normal: direction,
                        depth,
                    }),
                ))
            }
            Some(DrawingViewDerivationDto::Auxiliary {
                parent_view_id,
                reference,
                flipped,
                ..
            }) => {
                let parent = views
                    .iter()
                    .find(|p| p.id == *parent_view_id)
                    .ok_or("Derived view parent is missing")?;
                let (pd, _, _) = resolve(parent, views, scene, assembly, path)?;
                let anchor = |endpoint| DrawingTopologyAnchorRefDto {
                    occurrence_id: reference.occurrence_id,
                    body_id: reference.body_id,
                    edge_id: reference.edge_id,
                    edge_key: reference.edge_key.clone(),
                    endpoint,
                    fallback_point: [0.; 3],
                    circle_center: false,
                };
                let a = model_anchor(&anchor(DrawingEdgeEndpoint::Start), scene, assembly)?;
                let b = model_anchor(&anchor(DrawingEdgeEndpoint::End), scene, assembly)?;
                let edge = norm(std::array::from_fn(|i| b[i] - a[i]))?;
                let mut direction = norm(cross(edge, norm(pd)?))?;
                if *flipped {
                    direction = direction.map(|x| -x);
                }
                Ok((direction, norm(cross(direction, edge))?, None))
            }
            Some(
                DrawingViewDerivationDto::Detail { parent_view_id, .. }
                | DrawingViewDerivationDto::Broken { parent_view_id, .. },
            ) => {
                let parent = views
                    .iter()
                    .find(|p| p.id == *parent_view_id)
                    .ok_or("Derived view parent is missing")?;
                resolve(parent, views, scene, assembly, path)
            }
        };
        path.pop();
        result
    }
    let (direction, up, section_plane) = resolve(view, views, scene, assembly, &mut Vec::new())?;
    Ok(DrawingProjectionRequest {
        scope: view.scope,
        occurrence_ids: view.occurrence_ids.clone(),
        resolved_occurrences: None,
        body_ids: view.body_ids.clone(),
        direction,
        up,
        include_hidden: view.show_hidden_lines,
        include_tangent_edges: view.show_tangent_edges,
        deflection: (0.08 / view.scale).max(0.01),
        section_plane,
    })
}
fn paper_point(v: &DrawingViewDto, p: P, projection: &DrawingProjectionDto) -> P {
    let b = projection.bounds;
    [
        v.position[0] + (p[0] - (b[0] + b[2]) * 0.5) * v.scale,
        v.position[1] - (p[1] - (b[1] + b[3]) * 0.5) * v.scale,
    ]
}
fn circle_polyline(c: P, r: f64) -> Vec<P> {
    (0..=128)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 128.;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}
fn clip_view_polyline(
    v: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    points: &[P],
) -> Result<Vec<Vec<P>>, String> {
    let Some(derivation) = &v.derivation else {
        return Ok(vec![points.to_vec()]);
    };
    match derivation {
        DrawingViewDerivationDto::Detail { center, radius, .. } => {
            let c = paper_point(v, anchor_point(center, projection)?, projection);
            let r = radius * v.scale;
            let mut lines = Vec::new();
            for pair in points.windows(2) {
                let a = pair[0];
                let b = pair[1];
                let d = [b[0] - a[0], b[1] - a[1]];
                let o = [a[0] - c[0], a[1] - c[1]];
                let aa = d[0] * d[0] + d[1] * d[1];
                if aa < 1e-20 {
                    continue;
                }
                let bb = 2. * (o[0] * d[0] + o[1] * d[1]);
                let cc = o[0] * o[0] + o[1] * o[1] - r * r;
                let disc = bb * bb - 4. * aa * cc;
                if disc < 0. {
                    continue;
                }
                let lo = ((-bb - disc.sqrt()) / (2. * aa)).max(0.);
                let hi = ((-bb + disc.sqrt()) / (2. * aa)).min(1.);
                if hi > lo {
                    lines.push(vec![
                        [a[0] + lo * d[0], a[1] + lo * d[1]],
                        [a[0] + hi * d[0], a[1] + hi * d[1]],
                    ]);
                }
            }
            Ok(lines)
        }
        DrawingViewDerivationDto::Broken { axis, gap_mm, .. } => {
            let k = match axis {
                DrawingBreakAxis::Horizontal => 0,
                DrawingBreakAxis::Vertical => 1,
            };
            let lo = v.position[k] - gap_mm.max(3.) * 0.5;
            let hi = v.position[k] + gap_mm.max(3.) * 0.5;
            let mut lines = Vec::new();
            for pair in points.windows(2) {
                let a = pair[0];
                let b = pair[1];
                let delta = b[k] - a[k];
                let mut ts = vec![0., 1.];
                if delta.abs() > 1e-12 {
                    for edge in [lo, hi] {
                        let t = (edge - a[k]) / delta;
                        if t > 0. && t < 1. {
                            ts.push(t);
                        }
                    }
                }
                ts.sort_by(f64::total_cmp);
                for t in ts.windows(2) {
                    let mid = a[k] + (t[0] + t[1]) * 0.5 * delta;
                    if mid <= lo || mid >= hi {
                        lines.push(
                            t.iter()
                                .map(|t| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])])
                                .collect(),
                        );
                    }
                }
            }
            Ok(lines)
        }
        _ => Ok(vec![points.to_vec()]),
    }
}
fn hatch_section(
    p: &mut Paper,
    v: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    angle: f64,
    spacing: f64,
    style: &DrawingLineStyleDto,
) -> Result<(), String> {
    // Even/odd intersections with all section boundaries retain hollow regions.
    // Half-open edges prevent counting a shared vertex twice.
    let a = angle.to_radians();
    let u = [a.cos(), a.sin()];
    let n = [-a.sin(), a.cos()];
    let edges = projection
        .section
        .iter()
        .flat_map(|line| {
            line.points.windows(2).map(|pair| {
                [
                    paper_point(v, pair[0], projection),
                    paper_point(v, pair[1], projection),
                ]
            })
        })
        .collect::<Vec<_>>();
    if edges.is_empty() {
        return Ok(());
    }
    let dot2 = |p: P, b: P| p[0] * b[0] + p[1] * b[1];
    let min = edges
        .iter()
        .flatten()
        .map(|p| dot2(*p, n))
        .fold(f64::INFINITY, f64::min);
    let max = edges
        .iter()
        .flatten()
        .map(|p| dot2(*p, n))
        .fold(f64::NEG_INFINITY, f64::max);
    let start = (min / spacing).floor() as i64;
    let end = (max / spacing).ceil() as i64;
    if end - start > 20000 {
        return Err("Section hatch is too dense for the sheet scale".into());
    }
    for i in start..=end {
        let y = i as f64 * spacing;
        let mut xs = Vec::new();
        for [a, b] in &edges {
            let ay = dot2(*a, n);
            let by = dot2(*b, n);
            if (ay <= y && by > y) || (by <= y && ay > y) {
                let t = (y - ay) / (by - ay);
                xs.push(dot2(
                    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])],
                    u,
                ));
            }
        }
        xs.sort_by(f64::total_cmp);
        if xs.len() % 2 != 0 {
            return Err("Section boundary is open; cannot hatch a manufacturing drawing".into());
        }
        for pair in xs.chunks_exact(2) {
            p.line(
                pair.iter()
                    .map(|x| [x * u[0] + y * n[0], x * u[1] + y * n[1]])
                    .collect(),
                "HATCH",
                style,
            );
        }
    }
    Ok(())
}
fn anchor_point(a: &DrawingTopologyAnchorRefDto, p: &DrawingProjectionDto) -> Result<P, String> {
    if a.circle_center {
        return p
            .circles
            .iter()
            .find(|c| {
                c.occurrence_id == a.occurrence_id
                    && c.body_id == a.body_id
                    && c.edge_id == a.edge_id
                    && c.edge_key == a.edge_key
            })
            .map(|c| c.center)
            .ok_or_else(|| "Circular dimension anchor is stale or not normal to the view".into());
    }
    p.anchors
        .iter()
        .find(|r| {
            r.occurrence_id == a.occurrence_id
                && r.body_id == a.body_id
                && r.edge_id == a.edge_id
                && r.edge_key == a.edge_key
                && matches!(
                    (r.endpoint, a.endpoint),
                    (
                        crate::DrawingProjectionAnchorEndpoint::Start,
                        DrawingEdgeEndpoint::Start
                    ) | (
                        crate::DrawingProjectionAnchorEndpoint::End,
                        DrawingEdgeEndpoint::End
                    )
                )
        })
        .map(|r| r.point)
        .ok_or_else(|| "Dimension anchor is missing from current projection".into())
}
fn view_projection<'a>(
    id: u64,
    s: &'a DrawingSheetDto,
    p: &'a BTreeMap<u64, DrawingProjectionDto>,
) -> Result<(&'a DrawingViewDto, &'a DrawingProjectionDto), String> {
    Ok((
        s.views
            .iter()
            .find(|v| v.id == id)
            .ok_or("Dimension view missing")?,
        p.get(&id).ok_or("Dimension projection missing")?,
    ))
}
fn dimension_text(
    value: f64,
    precision: u8,
    prefix: &str,
    suffix: &str,
    p: &DrawingDimensionPresentationDto,
) -> Result<String, String> {
    if p.dual_units.is_some() {
        return Err("Native export does not yet support dual-unit dimension presentation".into());
    }
    let n = |x: f64| format!("{:.*}", precision as usize, x);
    let value = match p.tolerance.mode {
        DrawingDimensionToleranceMode::None => n(value),
        DrawingDimensionToleranceMode::Symmetric => {
            format!("{} ±{}", n(value), n(p.tolerance.upper.abs()))
        }
        DrawingDimensionToleranceMode::Deviation => format!(
            "{} {:+.*}/{:+.*}",
            n(value),
            precision as usize,
            p.tolerance.upper,
            precision as usize,
            p.tolerance.lower
        ),
        DrawingDimensionToleranceMode::Limits => format!(
            "{} / {}",
            n(value + p.tolerance.upper),
            n(value + p.tolerance.lower)
        ),
    };
    let mut text = format!(
        "{prefix}{value}{suffix}{}",
        if p.fit_class.is_empty() {
            String::new()
        } else {
            format!(" {}", p.fit_class)
        }
    );
    if p.reference {
        text = format!("({text})");
    }
    if p.basic {
        text = format!("[{text}]");
    }
    Ok(text)
}
fn arrows(p: &mut Paper, a: P, b: P, style: &DrawingSheetStyleDto) {
    let l = (b[0] - a[0]).hypot(b[1] - a[1]);
    if l < 1e-9 {
        return;
    }
    let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
    let size = style.arrow_size_mm.min(l / 3.);
    for (tip, sign) in [(a, 1.), (b, -1.)] {
        let base = [tip[0] + sign * u[0] * size, tip[1] + sign * u[1] * size];
        p.line(
            vec![
                [base[0] - u[1] * size * 0.3, base[1] + u[0] * size * 0.3],
                tip,
                [base[0] + u[1] * size * 0.3, base[1] - u[0] * size * 0.3],
            ],
            "DIMENSION",
            &style.dimension,
        );
    }
}
fn draw_annotation(
    p: &mut Paper,
    s: &DrawingSheetDto,
    projections: &BTreeMap<u64, DrawingProjectionDto>,
    a: &DrawingAnnotationDto,
) -> Result<(), String> {
    match a {
        DrawingAnnotationDto::Note{text,position,..}=>p.text(*position,text,s.style.text_height_mm),
        DrawingAnnotationDto::LinearDimension{view_id,first,second,mode,offset,prefix,suffix,precision,presentation,..}=> {

            let (v,pr)=view_projection(*view_id,s,projections)?;
let a=anchor_point(first,pr)?;
let b=anchor_point(second,pr)?;
            let value=match mode{DrawingLinearDimensionMode::Horizontal=>(b[0]-a[0]).abs(),DrawingLinearDimensionMode::Vertical=>(b[1]-a[1]).abs(),DrawingLinearDimensionMode::Aligned=>(b[0]-a[0]).hypot(b[1]-a[1])};
            if value<1e-9{return Err("Dimension has zero projected length".into());}
            let a=paper_point(v,a,pr);
let b=paper_point(v,b,pr);
let (c,d)=match mode{
                DrawingLinearDimensionMode::Horizontal=>([a[0],a[1]+offset],[b[0],a[1]+offset]),DrawingLinearDimensionMode::Vertical=>([a[0]+offset,a[1]],[a[0]+offset,b[1]]),DrawingLinearDimensionMode::Aligned=> {
let l=(b[0]-a[0]).hypot(b[1]-a[1]);
let n=[-(b[1]-a[1])/l,(b[0]-a[0])/l];([a[0]+n[0]*offset,a[1]+n[1]*offset],[b[0]+n[0]*offset,b[1]+n[1]*offset])}};
            p.line(vec![a,c],"EXTENSION",&s.style.extension);
p.line(vec![b,d],"EXTENSION",&s.style.extension);
p.line(vec![c,d],"DIMENSION",&s.style.dimension);arrows(p,c,d,&s.style);
            p.text([(c[0]+d[0])*0.5+1.,(c[1]+d[1])*0.5-1.5],dimension_text(value,*precision,prefix,suffix,presentation)?,s.style.text_height_mm);
        }
        DrawingAnnotationDto::RadialDimension{view_id,feature,mode,leader_angle_deg,offset,prefix,suffix,precision,presentation,..}=> {

            let (v,pr)=view_projection(*view_id,s,projections)?;
let c=pr.circles.iter().find(|c|c.occurrence_id==feature.occurrence_id&&c.body_id==feature.body_id&&c.edge_id==feature.edge_id&&c.edge_key==feature.edge_key).ok_or("Radial dimension reference is stale or not circular in this view")?;
            let center=paper_point(v,c.center,pr);
let r=c.radius*v.scale;
let a=leader_angle_deg.to_radians();
let u=[a.cos(),-a.sin()];
let edge=[center[0]+r*u[0],center[1]+r*u[1]];
let label=[center[0]+(r+offset)*u[0],center[1]+(r+offset)*u[1]];
            p.line(vec![center,edge,label],"DIMENSION",&s.style.dimension);arrows(p,edge,label,&s.style);
            let (value,symbol)=match mode{DrawingRadialDimensionMode::Radius=>(c.radius,"R"),DrawingRadialDimensionMode::Diameter=>(c.radius*2.,"Ø")};
p.text([label[0]+1.,label[1]-1.],dimension_text(value,*precision,&format!("{prefix}{symbol}"),suffix,presentation)?,s.style.text_height_mm);
        }
        DrawingAnnotationDto::AngularDimension{view_id,vertex,first,second,radius,prefix,suffix,precision,presentation,..}=> {

            let(v,pr)=view_projection(*view_id,s,projections)?;
let o=paper_point(v,anchor_point(vertex,pr)?,pr);
let a=paper_point(v,anchor_point(first,pr)?,pr);
let b=paper_point(v,anchor_point(second,pr)?,pr);
if (a[0]-o[0]).hypot(a[1]-o[1])<1e-9||(b[0]-o[0]).hypot(b[1]-o[1])<1e-9{return Err("Angular dimension has a collapsed arm".into());}
            let start=(a[1]-o[1]).atan2(a[0]-o[0]);
let end=(b[1]-o[1]).atan2(b[0]-o[0]);
let mut sweep=(end-start).rem_euclid(std::f64::consts::TAU);
if sweep>std::f64::consts::PI{sweep-=std::f64::consts::TAU;}
            let points=(0..=48).map(|i|{let t=start+sweep*i as f64/48.;[o[0]+radius*t.cos(),o[1]+radius*t.sin()]}).collect::<Vec<_>>();
p.line(vec![o,points[0]],"EXTENSION",&s.style.extension);
p.line(vec![o,points[48]],"EXTENSION",&s.style.extension);
let m=points[24];
p.line(points,"DIMENSION",&s.style.dimension);
p.text([m[0]+1.,m[1]-1.],dimension_text(sweep.abs().to_degrees(),*precision,prefix,&format!("°{suffix}"),presentation)?,s.style.text_height_mm);
        }
        _=>return Err(format!("Native sheet export does not yet support annotation {}; use the interactive drawing export for this sheet",a.id())),
    }
    Ok(())
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn svg(p: &Paper, font: &str) -> String {
    let [w, h] = p.size;
    let mut s=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\"><rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\n");
    for item in &p.items {
        match item {
            Primitive::Line {
                points,
                layer,
                width,
                dash,
            } => {
                let points = points
                    .iter()
                    .map(|p| format!("{:.5},{:.5}", p[0], p[1]))
                    .collect::<Vec<_>>()
                    .join(" ");
                let d = dash
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                writeln!(s,"<polyline data-layer=\"{layer}\" points=\"{points}\" fill=\"none\" stroke=\"#111\" stroke-width=\"{width}\" stroke-dasharray=\"{d}\"/>").unwrap();
            }
            Primitive::Text {
                point,
                value,
                height,
            } => {
                writeln!(s,"<text x=\"{:.5}\" y=\"{:.5}\" font-family=\"{}\" font-size=\"{height}\" fill=\"#111\">{}</text>",point[0],point[1],xml(font),xml(value)).unwrap();
            }
        }
    }
    s.push_str("</svg>\n");
    s
}
fn dxf_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\n' || c == '\r' {
                " ".into()
            } else if c == '\\' {
                "\\U+005C".into()
            } else if c.is_ascii() {
                c.to_string()
            } else {
                c.encode_utf16(&mut [0; 2])
                    .iter()
                    .map(|v| format!("\\U+{v:04X}"))
                    .collect()
            }
        })
        .collect()
}
fn dxf(p: &Paper) -> String {
    let mut s=String::from("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1021\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n70\n0\n");
    let mut styles = BTreeMap::new();
    for item in &p.items {
        if let Primitive::Line { layer, dash, .. } = item {
            if !dash.is_empty() {
                styles.insert(*layer, dash);
            }
        }
    }
    for (layer, dash) in styles {
        writeln!(
            s,
            "0\nLTYPE\n2\nNBS_{layer}\n70\n0\n3\n{layer}\n72\n65\n73\n{}\n40\n{}",
            dash.len(),
            dash.iter().sum::<f64>()
        )
        .unwrap();
        for (i, d) in dash.iter().enumerate() {
            writeln!(s, "49\n{}\n74\n0", if i % 2 == 0 { *d } else { -d }).unwrap();
        }
    }
    s.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    for item in &p.items {
        match item {
            Primitive::Line {
                points,
                layer,
                dash,
                width,
            } => {
                for pair in points.windows(2) {
                    writeln!(s,"0\nLINE\n8\n{layer}\n6\n{}\n370\n{}\n10\n{:.5}\n20\n{:.5}\n11\n{:.5}\n21\n{:.5}",if dash.is_empty(){"CONTINUOUS".into()}else{format!("NBS_{layer}")},(width*100.).round() as i32,pair[0][0],p.size[1]-pair[0][1],pair[1][0],p.size[1]-pair[1][1]).unwrap();
                }
            }
            Primitive::Text {
                point,
                value,
                height,
            } => {
                writeln!(
                    s,
                    "0\nTEXT\n8\nANNOTATION\n10\n{:.5}\n20\n{:.5}\n40\n{height}\n1\n{}",
                    point[0],
                    p.size[1] - point[1],
                    dxf_text(value)
                )
                .unwrap();
            }
        }
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture(length: f64) -> (DrawingDocumentDto, SolidSceneDto, DrawingProjectionDto) {
        let scene:SolidSceneDto=serde_json::from_value(json!({"bodies":[{"id":1,"name":"Rail","feature_id":1,"mesh":{"positions":[],"normals":[],"indices":[]},"faces":[],"edges":[{"id":1,"key":"bottom","points":[{"x":10.,"y":0.,"z":0.},{"x":10.+length,"y":0.,"z":0.}],"circle":null,"refinable":true}]}],"errors":[]})).unwrap();
        let mut manager = SketchManager::new();
        let mut doc=manager.drawing_command(serde_json::from_value(json!({"type":"create_sheet","arguments":{"name":"Rail drawing","format":"a4","orientation":"landscape"}})).unwrap()).unwrap();
        doc.sheets[0].views.push(serde_json::from_value(json!({"id":1,"name":"Front","kind":"front","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,70.],"scale":2.})).unwrap());
        doc.next_view_id = 2;
        let anchor = |end| json!({"body_id":1,"edge_id":1,"edge_key":"bottom","endpoint":end,"fallback_point":[999.,999.,999.]});
        doc.sheets[0].annotations.push(serde_json::from_value(json!({"kind":"linear_dimension","id":1,"view_id":1,"first":anchor("start"),"second":anchor("end"),"mode":"horizontal","offset":10.})).unwrap());
        doc.sheets[0].annotations.push(
            serde_json::from_value(
                json!({"kind":"note","id":2,"text":"<check & fit> Ø","position":[20.,25.]}),
            )
            .unwrap(),
        );
        doc.next_annotation_id = 3;
        let req = projection_request(
            &doc.sheets[0].views[0],
            &doc.sheets[0].views,
            &scene,
            &AssemblyDocumentDto::default(),
        )
        .unwrap();
        let mut projection:DrawingProjectionDto=serde_json::from_value(json!({"visible":[{"points":[[10.,0.],[10.+length,0.]]}],"hidden":[],"section":[],"bounds":[10.,0.,10.+length,0.]})).unwrap();
        projection.anchors = crate::drawing_projection_anchors(&scene, &req, &projection).unwrap();
        (doc, scene, projection)
    }
    #[test]
    fn export_measures_current_topology_and_centers_views_like_the_editor() {
        let (doc, scene, projection) = fixture(20.);
        let request = DrawingExportRequest {
            sheet_id: 1,
            format: DrawingExportFormat::Svg,
        };
        let export = || {
            export_sheet(
                &doc,
                &scene,
                &AssemblyDocumentDto::default(),
                &request,
                |_| Ok(projection.clone()),
            )
            .unwrap()
        };
        let text = export();
        assert_eq!(text, export());
        assert!(text.contains("80.00000,70.00000 120.00000,70.00000"));
        assert!(text.contains(">20.00</text>"));
        assert!(text.contains("&lt;check &amp; fit&gt; Ø"));
        let (doc, scene, projection) = fixture(25.);
        let edited = export_sheet(
            &doc,
            &scene,
            &AssemblyDocumentDto::default(),
            &request,
            |_| Ok(projection.clone()),
        )
        .unwrap();
        assert!(edited.contains(">25.00</text>"));
        assert!(!edited.contains("999.00000"));
        let dxf = export_sheet(
            &doc,
            &scene,
            &AssemblyDocumentDto::default(),
            &DrawingExportRequest {
                format: DrawingExportFormat::Dxf,
                ..request
            },
            |_| Ok(projection.clone()),
        )
        .unwrap();
        assert!(dxf.contains("$INSUNITS\n70\n4"));
        assert!(dxf.contains("\\U+00D8"));
        assert!(dxf.ends_with("0\nEOF\n"));
    }
    #[test]
    fn stale_dimensions_fail_instead_of_exporting_fallback_values() {
        let (doc, scene, mut projection) = fixture(20.);
        projection.anchors[0].edge_key = "replaced".into();
        let error = export_sheet(
            &doc,
            &scene,
            &AssemblyDocumentDto::default(),
            &DrawingExportRequest {
                sheet_id: 1,
                format: DrawingExportFormat::Svg,
            },
            |_| Ok(projection.clone()),
        )
        .unwrap_err();
        assert!(error.contains("missing from current projection"));
    }
    #[test]
    fn section_hatch_preserves_voids_and_rejects_open_boundaries() {
        let (doc, _, mut projection) = fixture(20.);
        let view = &doc.sheets[0].views[0];
        projection.bounds = [0., 0., 10., 10.];
        projection.section = serde_json::from_value(json!([
            {"points":[[0.,0.],[10.,0.],[10.,10.],[0.,10.],[0.,0.]]},
            {"points":[[3.,3.],[7.,3.],[7.,7.],[3.,7.],[3.,3.]]}
        ]))
        .unwrap();
        let mut p = Paper {
            size: [297., 210.],
            items: Vec::new(),
        };
        hatch_section(
            &mut p,
            view,
            &projection,
            0.,
            1.,
            &doc.sheets[0].style.hatch,
        )
        .unwrap();
        assert!(!p.items.is_empty());
        for item in p.items {
            if let Primitive::Line { points, .. } = item {
                let mid = [
                    (points[0][0] + points[1][0]) * 0.5,
                    (points[0][1] + points[1][1]) * 0.5,
                ];
                assert!(!(mid[0] > 96. && mid[0] < 104. && mid[1] > 66. && mid[1] < 74.));
            }
        }
        projection.section[0].points.pop();
        assert!(hatch_section(
            &mut Paper {
                size: [297., 210.],
                items: Vec::new()
            },
            view,
            &projection,
            0.,
            1.,
            &doc.sheets[0].style.hatch
        )
        .is_err());
    }
}
