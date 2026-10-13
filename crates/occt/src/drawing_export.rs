//! Deterministic engineering-sheet export. Both formats use the same paper
//! primitives and current exact projection; saved fallback coordinates never
//! substitute for lost topology. Files remain exports of editable drawing DTOs.
use crate::{DrawingProjectionDto, DrawingProjectionRequest, DrawingSectionPlaneDto};
use limo_cad_sketch::*;
use limo_cad_solid::SolidSceneDto;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;

#[cfg(test)]
use crate as occt;
#[cfg(test)]
#[path = "../tests/support/center_export.rs"]
mod center_fixture;
#[cfg(test)]
#[path = "../tests/support/straight_export.rs"]
mod straight_fixture;

mod advanced;
#[cfg(test)]
mod advanced_tests;
mod centers;
#[cfg(test)]
mod centers_tests;
mod cloud;
#[cfg(test)]
mod cloud_tests;
mod font;
mod graphics;
mod hole;
#[cfg(test)]
mod hole_tests;
mod section_graphics;
mod series;
#[cfg(test)]
mod series_tests;
mod source_graphics;
mod straight;
#[cfg(test)]
mod straight_tests;
mod text_outlines;
pub use text_outlines::{load_outline_fonts, resolve_svg_text};
mod title_block;
pub use graphics::{HatchPattern, PaperGraphicsBudget, PaperGraphicsLimits, PaperGraphicsUsage};
pub use section_graphics::{section_hatch, section_hatch_tiled};
pub use source_graphics::derived_source_graphics;
#[cfg(test)]
mod graphics_tests;

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
/// Borrowable paper-millimetre presentation shared with the native sheet.
/// This is deliberately not a serialized document or a second drawing model.
#[derive(Debug, PartialEq)]
pub enum PaperPrimitive {
    Line {
        points: Vec<P>,
        layer: &'static str,
        width: f64,
        dash: Vec<f64>,
    },
    Text {
        layer: &'static str,
        point: P,
        value: String,
        height: f64,
        centered: bool,
        rotation_deg: f64,
        fitted_width: Option<f64>,
    },
    Triangle {
        points: [P; 3],
        layer: &'static str,
    },
}
type Primitive = PaperPrimitive;
const TEXT_MASK: &str = "TEXT_MASK";
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
        self.aligned_text(point, value, height, false, 0.);
    }
    fn aligned_text(
        &mut self,
        point: P,
        value: impl Into<String>,
        height: f64,
        centered: bool,
        rotation_deg: f64,
    ) {
        let angle = rotation_deg.to_radians();
        for (i, line) in value.into().lines().enumerate() {
            let down = i as f64 * height * 1.4;
            self.items.push(Primitive::Text {
                layer: "ANNOTATION",
                point: [point[0] - down * angle.sin(), point[1] + down * angle.cos()],
                value: line.into(),
                height,
                centered,
                rotation_deg,
                fitted_width: None,
            });
        }
    }
    fn fitted_text(&mut self, point: P, value: impl Into<String>, height: f64, width: f64) {
        let value = value.into();
        let count = value
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(1)
            .max(1);

        self.text(point, value, height.min(width / (count as f64 * 0.65)));
    }

    fn title_cell(
        &mut self,
        field: &str,
        value: &str,
        origin: P,
        size: P,
        requested_height: f64,
    ) -> Result<(), String> {
        let fitted =
            title_block::fit_text(field, value, requested_height, size[0] - 3., size[1] - 2.)?;
        for (index, line) in fitted.lines.into_iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let width = title_block::text_width(&line, fitted.height);
            self.items.push(Primitive::Text {
                layer: "ANNOTATION",
                point: [
                    origin[0] + 1.5,
                    origin[1]
                        + 1.
                        + fitted.height * (0.85 + index as f64 * title_block::LINE_SPACING),
                ],
                value: line,
                height: fitted.height,
                centered: false,
                rotation_deg: 0.,
                fitted_width: Some(width),
            });
        }
        Ok(())
    }
}

struct Marks {
    entries: Vec<(usize, crate::drawing_presentation::layout::Motion, u32)>,
    next: u32,
}
impl Marks {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            next: 1,
        }
    }
    fn cover(
        &mut self,
        start: usize,
        end: usize,
        motion: crate::drawing_presentation::layout::Motion,
    ) {
        if start >= end {
            return;
        }
        let group = self.next;
        self.next += 1;
        for index in start..end {
            self.entries.push((index, motion, group));
        }
    }
    fn weld_labels(&mut self, start: usize, items: &[Primitive]) {
        let group = self.next;
        let mut boxes = Vec::new();
        for (index, item) in items.iter().enumerate().skip(start) {
            if let Primitive::Text {
                point,
                value,
                height,
                centered,
                fitted_width,
                ..
            } = item
            {
                boxes.push(text_bounds(
                    *point,
                    value,
                    *height,
                    *centered,
                    *fitted_width,
                ));
                self.entries.push((
                    index,
                    crate::drawing_presentation::layout::Motion::Weld,
                    group,
                ));
            }
        }
        if boxes.is_empty() {
            return;
        }
        self.next += 1;
        for (index, item) in items.iter().enumerate().skip(start) {
            let Primitive::Triangle { points, layer } = item else {
                continue;
            };
            if *layer != TEXT_MASK {
                continue;
            }
            let center = [
                (points[0][0] + points[1][0] + points[2][0]) / 3.,
                (points[0][1] + points[1][1] + points[2][1]) / 3.,
            ];
            if boxes.iter().any(|bounds| {
                center[0] >= bounds[0]
                    && center[0] <= bounds[2]
                    && center[1] >= bounds[1]
                    && center[1] <= bounds[3]
            }) {
                self.entries.push((
                    index,
                    crate::drawing_presentation::layout::Motion::Weld,
                    group,
                ));
            }
        }
    }
    fn motion(&self, index: usize) -> Option<(crate::drawing_presentation::layout::Motion, u32)> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.0 == index)
            .map(|entry| (entry.1, entry.2))
    }
}

fn text_bounds(
    point: P,
    value: &str,
    height: f64,
    centered: bool,
    fitted_width: Option<f64>,
) -> [f64; 4] {
    let align = if centered { 0. } else { 1. };
    let mut bounds = crate::drawing_presentation::text::label_bounds(point, value, height, align);
    if let Some(width) = fitted_width.filter(|width| width.is_finite() && *width >= 0.) {
        if centered {
            bounds[0] = point[0] - width * 0.5;
            bounds[2] = point[0] + width * 0.5;
        } else {
            bounds[0] = point[0];
            bounds[2] = point[0] + width;
        }
    }
    bounds
}

fn primitive_bounds(item: &Primitive) -> Option<[f64; 4]> {
    match item {
        Primitive::Text {
            point,
            value,
            height,
            centered,
            fitted_width,
            ..
        } => Some(text_bounds(
            *point,
            value,
            *height,
            *centered,
            *fitted_width,
        )),
        Primitive::Line { points, .. } => point_bounds(points),
        Primitive::Triangle { points, .. } => point_bounds(points),
    }
}

fn point_bounds(points: &[P]) -> Option<[f64; 4]> {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    if points.is_empty() {
        return None;
    }
    for point in points {
        if !point[0].is_finite() || !point[1].is_finite() {
            return None;
        }
        bounds[0] = bounds[0].min(point[0]);
        bounds[1] = bounds[1].min(point[1]);
        bounds[2] = bounds[2].max(point[0]);
        bounds[3] = bounds[3].max(point[1]);
    }
    Some(bounds)
}

fn shift_item(item: &mut Primitive, delta: [f64; 2]) {
    if delta[0].abs() < 1e-9 && delta[1].abs() < 1e-9 {
        return;
    }
    match item {
        Primitive::Line { points, .. } => {
            for point in points {
                point[0] += delta[0];
                point[1] += delta[1];
            }
        }
        Primitive::Text { point, .. } => {
            point[0] += delta[0];
            point[1] += delta[1];
        }
        Primitive::Triangle { points, .. } => {
            for point in points.iter_mut() {
                point[0] += delta[0];
                point[1] += delta[1];
            }
        }
    }
}

fn separate_collisions(items: &mut [Primitive], marks: &Marks) {
    use crate::drawing_presentation::layout::{Motion, Obstacle};
    let mut obstacles = Vec::new();
    let mut owners = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let Some(bounds) = primitive_bounds(item) else {
            continue;
        };
        if bounds.iter().any(|value| !value.is_finite()) {
            continue;
        }
        let (motion, group) = marks.motion(index).unwrap_or((Motion::Fixed, 0));
        if motion == Motion::Fixed && !matches!(item, Primitive::Text { .. }) {
            continue;
        }
        obstacles.push(Obstacle {
            bounds,
            motion,
            group,
        });
        owners.push(index);
    }
    let deltas = crate::drawing_presentation::layout::clearance_deltas(&obstacles);
    for (slot, index) in owners.into_iter().enumerate() {
        shift_item(&mut items[index], deltas[slot]);
    }
}

/// The caller owns the kernel/session. Supplying its projection closure avoids
/// another model load, filesystem side effects, or separate rendering process.
pub fn export_sheet(
    document: &DrawingDocumentDto,
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    request: &DrawingExportRequest,
    project: impl FnMut(&DrawingProjectionRequest) -> Result<DrawingProjectionDto, String>,
) -> Result<String, String> {
    export_sheet_with_units(
        document,
        scene,
        assembly,
        request,
        limo_cad_core::UnitSystem::Mm,
        project,
    )
}

/// Export using the existing document display units; geometry stays in millimetres.
/// The legacy host-neutral helper above defaults to millimetres. Hosts must pass
/// their document settings here rather than inventing a sheet-level unit system.
pub fn export_sheet_with_units(
    document: &DrawingDocumentDto,
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    request: &DrawingExportRequest,
    units: limo_cad_core::UnitSystem,
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
    limo_cad_sketch::drawing_topology::validate_drawing_sheet_topology(sheet, scene)?;
    let mut paper = Paper {
        size: sheet_size(sheet),
        items: Vec::new(),
    };
    let mut marks = Marks::new();
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
    draw_title_and_revisions(&mut paper, sheet, units, &mut marks)?;
    let mut projections = BTreeMap::new();
    let mut graphics_budget = PaperGraphicsBudget::default();
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
            paper.items.extend(section_hatch(
                view,
                &projection,
                &sheet.style.hatch,
                HatchPattern {
                    angle_deg: *hatch_angle_deg,
                    spacing_mm: *hatch_spacing_mm,
                },
                &mut graphics_budget,
            )?);
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
        let paper_height = (projection.bounds[3] - projection.bounds[1]).abs() * view.scale;
        let label_y = crate::drawing_presentation::layout::view_caption_baseline(
            view.position[1],
            paper_height,
            sheet.style.small_text_height_mm,
            dimension_caption_ink(sheet, view, &projection),
        );
        let label_y = centers::caption_baseline(label_y, sheet, view, &projection)?;
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
    draw_derived_sources(
        &mut paper,
        sheet,
        &projections,
        scene,
        assembly,
        &mut graphics_budget,
    )?;
    for annotation in &sheet.annotations {
        let start = paper.items.len();
        if let DrawingAnnotationDto::RevisionCloud {
            revision, points, ..
        } = annotation
        {
            let batch = cloud::draw(paper.size, revision, points, &mut graphics_budget)?;
            graphics_budget.append(&mut paper.items, batch)?;
            marks.cover(
                start,
                paper.items.len(),
                crate::drawing_presentation::layout::Motion::Cloud,
            );
        } else if matches!(
            annotation,
            DrawingAnnotationDto::ChainDimension { .. }
                | DrawingAnnotationDto::OrdinateDimension { .. }
        ) {
            let batch = series::draw(sheet, &projections, annotation, units, &mut graphics_budget)?;
            graphics_budget.append(&mut paper.items, batch)?;
        } else if advanced::supports(annotation) {
            let weld = matches!(annotation, DrawingAnnotationDto::WeldSymbol { .. });
            let batch =
                advanced::draw(sheet, &projections, annotation, units, &mut graphics_budget)?;
            graphics_budget.append(&mut paper.items, batch)?;
            if weld {
                marks.weld_labels(start, &paper.items);
            }
        } else {
            draw_annotation(&mut paper, sheet, &projections, annotation, units)?;
        }
    }
    if !sheet.bom.is_empty() {
        let start = paper.items.len();
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
        marks.cover(
            start,
            paper.items.len(),
            crate::drawing_presentation::layout::Motion::Table,
        );
    }
    separate_collisions(&mut paper.items, &marks);
    match request.format {
        DrawingExportFormat::Svg => Ok(svg(&paper, &sheet.style.font_family)),
        DrawingExportFormat::Dxf => dxf(&paper, &sheet.style.font_family),
    }
}

/// Parent markers share the editor's paper geometry. Resolve model topology
/// first: an edge-on circle still has an exact center even when it cannot be
/// represented as a circular curve in the parent projection.
fn draw_derived_sources(
    paper: &mut Paper,
    sheet: &DrawingSheetDto,
    projections: &BTreeMap<u64, DrawingProjectionDto>,
    scene: &SolidSceneDto,
    assembly: &AssemblyDocumentDto,
    budget: &mut PaperGraphicsBudget,
) -> Result<(), String> {
    for child in &sheet.views {
        paper.items.extend(derived_source_graphics(
            child,
            sheet,
            |id| projections.get(&id),
            scene,
            assembly,
            budget,
        )?);
    }
    Ok(())
}

fn source_direction(a: P, b: P) -> Result<P, String> {
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    if length < 1e-7 {
        return Err("Derived source direction is degenerate in its parent view".into());
    }
    Ok([(b[0] - a[0]) / length, (b[1] - a[1]) / length])
}

fn section_source_extent(
    a: P,
    b: P,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
) -> Result<[P; 2], String> {
    let u = source_direction(a, b)?;
    let mut low = f64::NEG_INFINITY;
    let mut high = f64::INFINITY;
    for axis in 0..2 {
        let extent =
            (projection.bounds[axis + 2] - projection.bounds[axis]) * view.scale * 0.5 + 4.;
        let min = view.position[axis] - extent;
        let max = view.position[axis] + extent;
        if u[axis].abs() < 1e-10 {
            if a[axis] < min || a[axis] > max {
                return Err("Section cutting plane misses its parent view".into());
            }
        } else {
            let first = (min - a[axis]) / u[axis];
            let second = (max - a[axis]) / u[axis];
            low = low.max(first.min(second));
            high = high.min(first.max(second));
        }
    }
    if low >= high {
        return Err("Section cutting plane misses its parent view".into());
    }
    Ok([low, high].map(|t| [a[0] + u[0] * t, a[1] + u[1] * t]))
}

fn draw_title_and_revisions(
    paper: &mut Paper,
    sheet: &DrawingSheetDto,
    units: limo_cad_core::UnitSystem,
    marks: &mut Marks,
) -> Result<(), String> {
    let [w, h] = paper.size;
    let width = 180_f64.min(w - 20.);
    let x = w - 10. - width;
    let y = h - 54.;
    let title = &sheet.title_block;
    let small = sheet.style.small_text_height_mm;
    paper.line(vec![[x, y], [w - 10., y]], "BORDER", &sheet.style.dimension);
    paper.line(vec![[x, y], [x, h - 10.]], "BORDER", &sheet.style.dimension);
    for offset in [7., 12., 16., 23., 30., 34., 39.] {
        paper.line(
            vec![[x, y + offset], [w - 10., y + offset]],
            "BORDER",
            &sheet.style.dimension,
        );
    }
    paper.title_cell(
        "title",
        if title.title.is_empty() {
            &sheet.name
        } else {
            &title.title
        },
        [x, y],
        [width, 7.],
        sheet.style.text_height_mm,
    )?;
    paper.title_cell(
        "drawing number / revision / sheet name",
        &format!(
            "DRAWING: {}   REV: {}   SHEET: {}",
            title.drawing_number, title.revision, sheet.name
        ),
        [x, y + 7.],
        [width, 5.],
        small,
    )?;
    let method = match sheet.projection_method {
        DrawingProjectionMethod::FirstAngle => "FIRST ANGLE",
        DrawingProjectionMethod::ThirdAngle => "THIRD ANGLE",
    };
    paper.title_cell(
        "standard / projection / release status",
        &format!(
            "DIMENSIONS: {}   {:?}   {method}   RELEASE: {:?}",
            crate::drawing_presentation::text::unit_label(units),
            sheet.standard,
            sheet.release.status
        ),
        [x, y + 12.],
        [width, 4.],
        small,
    )?;
    paper.title_cell(
        "material / finish",
        &format!("MATERIAL: {}   FINISH: {}", title.material, title.finish),
        [x, y + 16.],
        [width, 7.],
        small,
    )?;
    let tolerance = tolerance_note(&sheet.tolerance_note);
    paper.title_cell(
        "tolerance note",
        if tolerance.is_empty() {
            "TOLERANCES: AS SPECIFIED"
        } else {
            &tolerance
        },
        [x, y + 23.],
        [width, 7.],
        small,
    )?;
    paper.title_cell(
        "company",
        &format!("COMPANY: {}", title.company),
        [x, y + 30.],
        [width, 4.],
        small,
    )?;
    paper.title_cell(
        "author / checked by / approved by",
        &format!(
            "DRAWN: {}   CHECKED: {}   APPROVED: {}",
            title.author, title.checked_by, title.approved_by
        ),
        [x, y + 34.],
        [width, 5.],
        small,
    )?;
    paper.title_cell(
        "released revision / date",
        &format!(
            "RELEASED REVISION: {}   DATE: {}",
            sheet.release.released_revision, sheet.release.released_at
        ),
        [x, y + 39.],
        [width, 5.],
        small,
    )?;

    if let Some([rx, ry]) = sheet.revision_table_position {
        let table_start = paper.items.len();
        let rw = 220_f64.min(w - 10. - rx);
        let bottom = ry + 6. + sheet.revisions.len() as f64 * 15.;
        if rw < 40. || rx < 10. || ry < 10. || bottom > h - 10. {
            return Err("Revision table does not fit within the sheet border".into());
        }
        paper.fitted_text(
            [rx + 2., ry + 4.],
            "REVISION HISTORY — description, responsibility and release",
            small,
            rw - 4.,
        );
        for (i, revision) in sheet.revisions.iter().enumerate() {
            let top = ry + 6. + i as f64 * 15.;
            paper.line(
                vec![[rx, top], [rx + rw, top]],
                "REVISION",
                &sheet.style.dimension,
            );
            paper.fitted_text(
                [rx + 2., top + 4.],
                format!(
                    "{}   {}   {}   CHANGE ORDER: {}",
                    revision.revision, revision.date, revision.description, revision.change_order
                ),
                small,
                rw - 4.,
            );
            paper.fitted_text(
                [rx + 2., top + 8.5],
                format!(
                    "DRAWN: {}   CHECKED: {}   APPROVED: {}",
                    revision.author, revision.checked_by, revision.approved_by
                ),
                small,
                rw - 4.,
            );
            paper.fitted_text(
                [rx + 2., top + 13.],
                format!("STATUS: {:?}", revision.status),
                small,
                rw - 4.,
            );
        }
        marks.cover(
            table_start,
            paper.items.len(),
            crate::drawing_presentation::layout::Motion::Table,
        );
    }
    Ok(())
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
    type ProjectionBasis = ([f64; 3], [f64; 3], Option<DrawingSectionPlaneDto>);
    fn resolve(
        v: &DrawingViewDto,
        views: &[DrawingViewDto],
        scene: &SolidSceneDto,
        assembly: &AssemblyDocumentDto,
        path: &mut Vec<u64>,
    ) -> Result<ProjectionBasis, String> {
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
                // The cut line is drawn in the parent projection. Anchor
                // depths may differ; they must not tilt the child page basis.
                let pd = norm(pd)?;
                let delta = std::array::from_fn(|i| b[i] - a[i]);
                let depth = dot(delta, pd);
                let edge = norm(std::array::from_fn(|i| delta[i] - depth * pd[i]))?;
                let mut direction = norm(cross(edge, pd))?;
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
                    topology_signature: reference.topology_signature.clone(),
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
        deflection: (0.01 / view.scale).max(0.0001),
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

fn dimension_caption_ink(
    sheet: &DrawingSheetDto,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
) -> Option<f64> {
    let paper_height = (projection.bounds[3] - projection.bounds[1]).abs() * view.scale;
    let view_bottom = view.position[1] + paper_height * 0.5;
    let half = (projection.bounds[2] - projection.bounds[0]).abs() * view.scale * 0.5;
    let left = view.position[0] - half;
    let right = view.position[0] + half;
    let mut ink = None;
    for annotation in &sheet.annotations {
        let DrawingAnnotationDto::LinearDimension {
            view_id,
            first,
            second,
            offset,
            mode,
            ..
        } = annotation
        else {
            continue;
        };
        if *view_id != view.id {
            continue;
        }
        let Ok(start) = anchor_point(first, projection) else {
            continue;
        };
        let Ok(end) = anchor_point(second, projection) else {
            continue;
        };
        let start = paper_point(view, start, projection);
        let end = paper_point(view, end, projection);
        let Some(span) = crate::drawing_presentation::geometry::dimension_span(
            *mode, start, end, *offset, view.scale,
        ) else {
            continue;
        };
        let x0 = span.start[0].min(span.end[0]);
        let x1 = span.start[0].max(span.end[0]);
        let y1 = span.start[1].max(span.end[1]);
        if y1 < view_bottom - 0.5 || x1 < left - 8. || x0 > right + 8. {
            continue;
        }
        let y = crate::drawing_presentation::layout::dimension_ink_y(
            span.start[1],
            span.end[1],
            sheet.style.dimension.width_mm,
            y1 + 1e-6 >= start[1].max(end[1]),
        );
        ink = Some(ink.unwrap_or(y).max(y));
    }
    ink
}
fn circle_polyline(c: P, r: f64) -> Vec<P> {
    (0..=128)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 128.;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}
/// Paper-space circular boundary of a detail view, resolved from the current
/// projection rather than the reference's diagnostic fallback coordinates.
/// Source graphics additionally validate against current model topology before
/// a whole sheet is published; this helper checks the projection signature too.
pub fn detail_clip_circle(
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
) -> Result<Option<([f64; 2], f64)>, String> {
    let Some(DrawingViewDerivationDto::Detail { center, radius, .. }) = &view.derivation else {
        return Ok(None);
    };
    if projection
        .topology_signatures
        .get(&center.body_id.0.to_string())
        != center.topology_signature.as_ref()
    {
        return Err("Detail view reference has a stale topology signature".into());
    }
    let center = paper_point(view, anchor_point(center, projection)?, projection);
    let radius = radius * view.scale;
    if center.iter().any(|value| !value.is_finite()) || !radius.is_finite() || radius <= 0. {
        return Err("Detail view boundary lies outside finite paper coordinates".into());
    }
    Ok(Some((center, radius)))
}
fn clip_view_polyline(
    v: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    points: &[P],
) -> Result<Vec<Vec<P>>, String> {
    clip_view_polyline_with_detail(v, points, detail_clip_circle(v, projection)?)
}
/// Clip artwork and pick strokes using the same saved view mask. Resolve the
/// detail circle once per view so repeated edges do not rescan all anchors.
/// Associative reference endpoints remain unchanged.
pub fn clip_view_polyline_with_detail(
    v: &DrawingViewDto,
    points: &[P],
    detail_circle: Option<(P, f64)>,
) -> Result<Vec<Vec<P>>, String> {
    let Some(derivation) = &v.derivation else {
        return Ok(vec![points.to_vec()]);
    };
    match derivation {
        DrawingViewDerivationDto::Detail { .. } => {
            let (c, r) = detail_circle.ok_or("Detail boundary missing")?;
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
#[cfg(test)]
fn hatch_section(
    paper: &mut Paper,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    angle: f64,
    spacing: f64,
    style: &DrawingLineStyleDto,
) -> Result<(), String> {
    paper.items.extend(section_hatch(
        view,
        projection,
        style,
        HatchPattern {
            angle_deg: angle,
            spacing_mm: spacing,
        },
        &mut PaperGraphicsBudget::default(),
    )?);
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
    presentation: &DrawingDimensionPresentationDto,
    units: limo_cad_core::UnitSystem,
) -> String {
    let mut format = presentation.clone();
    format.basic = false;
    crate::drawing_presentation::text::dimension(value, precision, prefix, suffix, units, &format)
}
fn basic_label_rect(value: &str, height: f64, centered: bool) -> [f64; 4] {
    let width = value.chars().count() as f64 * height * 0.65;
    let left = if centered { -width / 2. } else { 0. };
    [left - 1., -height - 1., left + width + 1., 1.]
}

fn basic_label_corners(point: P, value: &str, height: f64, centered_angle: Option<f64>) -> [P; 4] {
    let [left, top, right, bottom] = basic_label_rect(value, height, centered_angle.is_some());
    let angle = centered_angle.unwrap_or(0.).to_radians();
    [[left, top], [right, top], [right, bottom], [left, bottom]].map(|[x, y]| {
        [
            point[0] + x * angle.cos() - y * angle.sin(),
            point[1] + x * angle.sin() + y * angle.cos(),
        ]
    })
}

fn paper_label_mask(paper: &mut Paper, corners: [P; 4]) {
    for points in [
        [corners[0], corners[1], corners[2]],
        [corners[0], corners[2], corners[3]],
    ] {
        paper.items.push(Primitive::Triangle {
            points,
            layer: TEXT_MASK,
        });
    }
}

/// Paint the native angular label's paper mask before its basic frame/text.
/// Two existing triangle primitives preserve order in both SVG and DXF; they
/// have explicit white color intent rather than relying on the current theme.
fn angular_label_mask(
    paper: &mut Paper,
    point: P,
    value: &str,
    presentation: &DrawingDimensionPresentationDto,
    style: &DrawingSheetStyleDto,
) {
    let mut bounds =
        crate::drawing_presentation::text::label_bounds(point, value, style.text_height_mm, 0.);
    if presentation.basic {
        let basic = basic_label_rect(value, style.text_height_mm, true);
        bounds[0] = bounds[0].min(point[0] + basic[0]);
        bounds[1] = bounds[1].min(point[1] + basic[1]);
        bounds[2] = bounds[2].max(point[0] + basic[2]);
        bounds[3] = bounds[3].max(point[1] + basic[3]);
    }
    let [left, top, right, bottom] = bounds;
    paper_label_mask(
        paper,
        [[left, top], [right, top], [right, bottom], [left, bottom]],
    );
}
fn dimension_label(
    p: &mut Paper,
    point: P,
    value: String,
    presentation: &DrawingDimensionPresentationDto,
    style: &DrawingSheetStyleDto,
    centered_angle: Option<f64>,
) {
    let height = style.text_height_mm;
    if presentation.basic {
        let corners = basic_label_corners(point, &value, height, centered_angle);
        p.line(
            vec![corners[0], corners[1], corners[2], corners[3], corners[0]],
            "DIMENSION",
            &style.dimension,
        );
    }
    p.aligned_text(
        point,
        value,
        height,
        centered_angle.is_some(),
        centered_angle.unwrap_or(0.),
    );
}
fn arrow(p: &mut Paper, tip: P, toward: P, style: &DrawingSheetStyleDto) {
    let length = (toward[0] - tip[0]).hypot(toward[1] - tip[1]);
    if length < 1e-9 {
        return;
    }
    let u = [(toward[0] - tip[0]) / length, (toward[1] - tip[1]) / length];
    let size = style.arrow_size_mm.min(length / 3.);
    let base = [tip[0] + u[0] * size, tip[1] + u[1] * size];
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
fn arrows(p: &mut Paper, a: P, b: P, style: &DrawingSheetStyleDto) {
    arrow(p, a, b, style);
    arrow(p, b, a, style);
}
fn draw_annotation(
    paper: &mut Paper,
    sheet: &DrawingSheetDto,
    projections: &BTreeMap<u64, DrawingProjectionDto>,
    annotation: &DrawingAnnotationDto,
    units: limo_cad_core::UnitSystem,
) -> Result<(), String> {
    let style = &sheet.style;
    match annotation {
        DrawingAnnotationDto::HoleNote { .. } => hole::draw(paper, sheet, projections, annotation, units)?,
        DrawingAnnotationDto::CenterMark { .. } | DrawingAnnotationDto::CenterLine { .. } => {
            centers::draw(paper, sheet, projections, annotation)?;
        }
        DrawingAnnotationDto::LineDimension { .. } | DrawingAnnotationDto::PointLineDimension { .. } => {
            straight::draw(paper, sheet, projections, annotation, units)?;
        }
        DrawingAnnotationDto::Note { text, position, .. } => {
            paper.text(*position, text, style.text_height_mm);
        }
        DrawingAnnotationDto::LinearDimension {
            view_id, first, second, mode, offset, prefix, suffix, precision, presentation, ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            let first = anchor_point(first, projection)?;
            let second = anchor_point(second, projection)?;
            let value = match mode {
                DrawingLinearDimensionMode::Horizontal => (second[0] - first[0]).abs(),
                DrawingLinearDimensionMode::Vertical => (second[1] - first[1]).abs(),
                DrawingLinearDimensionMode::Aligned => (second[0] - first[0]).hypot(second[1] - first[1]),
            };
            if value < 1e-9 {
                return Err("Dimension has zero projected length".into());
            }
            let a = paper_point(view, first, projection);
            let b = paper_point(view, second, projection);
            let (c, d) = match mode {
                DrawingLinearDimensionMode::Horizontal => ([a[0], a[1] + offset], [b[0], a[1] + offset]),
                DrawingLinearDimensionMode::Vertical => ([a[0] + offset, a[1]], [a[0] + offset, b[1]]),
                DrawingLinearDimensionMode::Aligned => {
                    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
                    let normal = [-(b[1] - a[1]) / length, (b[0] - a[0]) / length];
                    ([a[0] + normal[0] * offset, a[1] + normal[1] * offset],
                     [b[0] + normal[0] * offset, b[1] + normal[1] * offset])
                }
            };
            paper.line(vec![a, c], "EXTENSION", &style.extension);
            paper.line(vec![b, d], "EXTENSION", &style.extension);
            paper.line(vec![c, d], "DIMENSION", &style.dimension);
            arrows(paper, c, d, style);



            let mut angle = (d[1] - c[1]).atan2(d[0] - c[0]).to_degrees();
            if angle > 90. { angle -= 180.; }
            if angle < -90. { angle += 180.; }
            let radians = angle.to_radians();
            dimension_label(
                paper, [(c[0] + d[0]) * 0.5 + 1.5 * radians.sin(), (c[1] + d[1]) * 0.5 - 1.5 * radians.cos()],
                dimension_text(value, *precision, prefix, suffix, presentation, units), presentation, style,
                Some(angle),
            );
        }
        DrawingAnnotationDto::RadialDimension {
            view_id, feature, mode, leader_angle_deg, offset, prefix, suffix, precision, presentation, ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            let circle = projection.circles.iter().find(|circle| {
                circle.occurrence_id == feature.occurrence_id
                    && circle.body_id == feature.body_id
                    && circle.edge_id == feature.edge_id
                    && circle.edge_key == feature.edge_key
            }).ok_or("Radial dimension reference is stale or not circular in this view")?;
            let center = paper_point(view, circle.center, projection);
            let radius = circle.radius * view.scale;
            let angle = leader_angle_deg.to_radians();
            let direction = [angle.cos(), -angle.sin()];
            let edge = [center[0] + radius * direction[0], center[1] + radius * direction[1]];
            let label = [center[0] + (radius + offset) * direction[0], center[1] + (radius + offset) * direction[1]];
            paper.line(vec![edge, label], "DIMENSION", &style.dimension);
            arrow(paper, edge, label, style);
            let (value, symbol) = match mode {
                DrawingRadialDimensionMode::Radius => (circle.radius, "R"),
                DrawingRadialDimensionMode::Diameter => (circle.radius * 2., "Ø"),
            };
            dimension_label(
                paper, [label[0] + 1., label[1] - 1.],
                dimension_text(value, *precision, &format!("{prefix}{symbol}"), suffix, presentation, units),
                presentation, style, None,
            );
        }
        DrawingAnnotationDto::AngularDimension {
            view_id, vertex, first, second, radius, prefix, suffix, precision, presentation, ..
        } => {
            let (view, projection) = view_projection(*view_id, sheet, projections)?;
            let origin = paper_point(view, anchor_point(vertex, projection)?, projection);
            let first = paper_point(view, anchor_point(first, projection)?, projection);
            let second = paper_point(view, anchor_point(second, projection)?, projection);
            if (first[0] - origin[0]).hypot(first[1] - origin[1]) < 1e-9
                || (second[0] - origin[0]).hypot(second[1] - origin[1]) < 1e-9 {
                return Err("Angular dimension has a collapsed arm".into());
            }
            let start = (first[1] - origin[1]).atan2(first[0] - origin[0]);
            let end = (second[1] - origin[1]).atan2(second[0] - origin[0]);
            let mut sweep = (end - start).rem_euclid(std::f64::consts::TAU);
            if sweep > std::f64::consts::PI {
                sweep -= std::f64::consts::TAU;
            }
            let points = (0..=48).map(|index| {
                let angle = start + sweep * index as f64 / 48.;
                [origin[0] + radius * angle.cos(), origin[1] + radius * angle.sin()]
            }).collect::<Vec<_>>();
            paper.line(vec![origin, points[0]], "EXTENSION", &style.extension);
            paper.line(vec![origin, points[48]], "EXTENSION", &style.extension);
            let middle = points[24];
            arrow(paper, points[0], points[8], style);
            arrow(paper, points[48], points[40], style);
            paper.line(points, "DIMENSION", &style.dimension);
            dimension_label(
                paper, [middle[0] + 1., middle[1] - 1.],
                {
                    let mut format = presentation.clone();
                    format.basic = false;
                    crate::drawing_presentation::text::angular(sweep.abs().to_degrees(), *precision, prefix, suffix, &format)
                },
                presentation, style, None,
            );
        }
        _ => return Err(format!(
            "Native sheet export does not yet support annotation {}; use the interactive drawing export for this sheet",
            annotation.id(),
        )),
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
                let ink = if layer.starts_with("CENTER") {
                    "#356170"
                } else if *layer == "REVISION" {
                    crate::drawing_presentation::cloud::COLOR
                } else {
                    "#111"
                };
                writeln!(s,"<polyline data-layer=\"{layer}\" points=\"{points}\" fill=\"none\" stroke=\"{ink}\" stroke-width=\"{width}\" stroke-dasharray=\"{d}\"/>").unwrap();
            }
            Primitive::Text {
                layer,
                point,
                value,
                height,
                centered,
                rotation_deg,
                fitted_width,
            } => {
                let anchor = if *centered {
                    " text-anchor=\"middle\""
                } else {
                    ""
                };
                let rotation = if *rotation_deg != 0. {
                    format!(
                        " transform=\"rotate({rotation_deg:.5} {:.5} {:.5})\"",
                        point[0], point[1]
                    )
                } else {
                    String::new()
                };
                let fit = fitted_width.map_or_else(String::new, |width| {
                    format!(" textLength=\"{width:.5}\" lengthAdjust=\"spacingAndGlyphs\"")
                });
                let ink = if *layer == "REVISION" {
                    crate::drawing_presentation::cloud::COLOR
                } else {
                    "#111"
                };
                writeln!(s,"<text xml:space=\"preserve\" x=\"{:.5}\" y=\"{:.5}\" font-family=\"{}\" font-size=\"{height}\" fill=\"{ink}\"{anchor}{rotation}{fit}>{}</text>",point[0],point[1],xml(font),xml(value)).unwrap();
            }
            Primitive::Triangle { points, layer } => {
                let points = points
                    .iter()
                    .map(|p| format!("{:.5},{:.5}", p[0], p[1]))
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(
                    s,
                    "<polygon data-layer=\"{layer}\" points=\"{points}\" fill=\"{}\"/>",
                    if *layer == TEXT_MASK { "white" } else { "#111" },
                )
                .unwrap();
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
            } else {
                c.to_string()
            }
        })
        .collect()
}
fn dxf_lineweight(width_mm: f64) -> i32 {
    const WEIGHTS: [i32; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    let requested = width_mm * 100.;
    WEIGHTS
        .into_iter()
        .min_by(|a, b| {
            (f64::from(*a) - requested)
                .abs()
                .total_cmp(&(f64::from(*b) - requested).abs())
        })
        .unwrap()
}

fn dxf(p: &Paper, font_family: &str) -> Result<String, String> {
    let font_family = font::family(font_family)?;
    let mut styles = BTreeMap::new();
    let mut layers = std::collections::BTreeSet::from(["0"]);
    for item in &p.items {
        match item {
            Primitive::Line { layer, dash, .. } => {
                layers.insert(*layer);
                if !dash.is_empty() {
                    styles.insert(*layer, dash);
                }
            }
            Primitive::Triangle { layer, .. } => {
                layers.insert(*layer);
            }
            Primitive::Text { layer, .. } => {
                layers.insert(*layer);
            }
        }
    }
    let layer_table_handle = styles.len() + 4;
    let style_table_handle = layer_table_handle + layers.len() + 1;
    let mut s = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1021\n9\n$INSUNITS\n70\n4\n9\n$HANDSEED\n5\n{:X}\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n5\n2\n330\n0\n100\nAcDbSymbolTable\n70\n{}\n",
        style_table_handle + 4,
        styles.len() + 1,
    );
    s.push_str("0\nLTYPE\n5\n3\n330\n2\n100\nAcDbSymbolTableRecord\n100\nAcDbLinetypeTableRecord\n2\nCONTINUOUS\n70\n0\n3\nSolid line\n72\n65\n73\n0\n40\n0\n");
    for (index, (layer, dash)) in styles.iter().enumerate() {
        writeln!(
            s,
            "0\nLTYPE\n5\n{:X}\n330\n2\n100\nAcDbSymbolTableRecord\n100\nAcDbLinetypeTableRecord\n2\nNBS_{layer}\n70\n0\n3\n{layer}\n72\n65\n73\n{}\n40\n{}",
            index + 4,
            dash.len(),
            dash.iter().sum::<f64>()
        )
        .unwrap();
        for (i, d) in dash.iter().enumerate() {
            writeln!(s, "49\n{}\n74\n0", if i % 2 == 0 { *d } else { -d }).unwrap();
        }
    }
    writeln!(s, "0\nENDTAB\n0\nTABLE\n2\nLAYER\n5\n{layer_table_handle:X}\n330\n0\n100\nAcDbSymbolTable\n70\n{}", layers.len()).unwrap();
    for (index, layer) in layers.iter().enumerate() {
        writeln!(s,
            "0\nLAYER\n5\n{:X}\n330\n{layer_table_handle:X}\n100\nAcDbSymbolTableRecord\n100\nAcDbLayerTableRecord\n2\n{layer}\n70\n0\n62\n7\n6\n{}",
            layer_table_handle + index + 1,
            if styles.contains_key(*layer) { format!("NBS_{layer}") } else { "CONTINUOUS".into() },
        ).unwrap();
        if *layer == TEXT_MASK {
            s.push_str("420\n16777215\n");
        } else if layer.starts_with("CENTER") {
            s.push_str("420\n3498352\n");
        } else if *layer == "REVISION" {
            s.push_str("420\n12860237\n");
        }
    }
    s.push_str("0\nENDTAB\n");
    font::tables(&mut s, style_table_handle, &font_family);
    s.push_str("0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    for item in &p.items {
        match item {
            Primitive::Line {
                points,
                layer,
                dash,
                width,
            } => {
                if points.len() > 2 && !dash.is_empty() {
                    writeln!(
                        s,
                        "0\nLWPOLYLINE\n100\nAcDbEntity\n8\n{layer}\n6\nNBS_{layer}\n370\n{}\n100\nAcDbPolyline\n90\n{}\n70\n128",
                        dxf_lineweight(*width), points.len()
                    ).unwrap();
                    for point in points {
                        writeln!(s, "10\n{:.5}\n20\n{:.5}", point[0], p.size[1] - point[1])
                            .unwrap();
                    }
                    continue;
                }
                for pair in points.windows(2) {
                    writeln!(s,"0\nLINE\n8\n{layer}\n6\n{}\n370\n{}\n10\n{:.5}\n20\n{:.5}\n11\n{:.5}\n21\n{:.5}",if dash.is_empty(){"CONTINUOUS".into()}else{format!("NBS_{layer}")},dxf_lineweight(*width),pair[0][0],p.size[1]-pair[0][1],pair[1][0],p.size[1]-pair[1][1]).unwrap();
                }
            }
            Primitive::Text {
                layer,
                point,
                value,
                height,
                centered,
                rotation_deg,
                fitted_width,
            } => {
                text_outlines::write_text(
                    &mut s,
                    p.size[1],
                    *point,
                    value,
                    *height,
                    *centered,
                    *rotation_deg,
                    *fitted_width,
                    layer,
                    &font_family,
                )?;
            }
            Primitive::Triangle { points, layer } => {
                writeln!(s, "0\nSOLID\n8\n{layer}").unwrap();
                if *layer == TEXT_MASK {
                    s.push_str("420\n16777215\n");
                }
                for (index, point) in [points[0], points[1], points[2], points[2]]
                    .iter()
                    .enumerate()
                {
                    writeln!(
                        s,
                        "{}\n{:.5}\n{}\n{:.5}",
                        10 + index,
                        point[0],
                        20 + index,
                        p.size[1] - point[1]
                    )
                    .unwrap();
                }
            }
        }
    }
    s.push_str("0\nENDSEC\n");
    s.push_str("0\nEOF\n");
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn dxf_tables_have_owned_unique_handles_actual_counts_and_all_referenced_layers() {
        let tags = |content: &str| -> Vec<(String, String)> {
            content
                .lines()
                .collect::<Vec<_>>()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| (pair[0].into(), pair[1].into()))
                .collect()
        };
        for dashed in [false, true] {
            let mut paper = Paper {
                size: [297., 210.],
                items: vec![],
            };
            if dashed {
                for layer in ["HIDDEN", "CENTER", "HIDDEN"] {
                    paper.line(
                        vec![[1., 2.], [3., 4.]],
                        layer,
                        &DrawingLineStyleDto {
                            width_mm: 0.25,
                            dash_mm: vec![4., 2.],
                        },
                    );
                }
                paper.text([10., 20.], "Visible label", 3.5);
                paper.items.push(Primitive::Triangle {
                    points: [[1., 1.], [2., 1.], [2., 2.]],
                    layer: TEXT_MASK,
                });
            }
            let output = dxf(&paper, "Arial, Helvetica, sans-serif").unwrap();
            assert_eq!(
                output,
                dxf(&paper, "Arial, Helvetica, sans-serif").unwrap(),
                "Handle allocation must remain deterministic"
            );
            let tags = tags(&output);
            let seed = tags
                .windows(2)
                .find(|pair| pair[0] == ("9".into(), "$HANDSEED".into()))
                .unwrap();
            assert_eq!(seed[1].0, "5");
            let seed = usize::from_str_radix(&seed[1].1, 16).unwrap();
            let mut records: Vec<Vec<&(String, String)>> = vec![];
            for pair in &tags {
                if pair.0 == "0" {
                    records.push(vec![]);
                }
                records.last_mut().unwrap().push(pair);
            }
            let field = |record: &Vec<&(String, String)>, code: &str| {
                record
                    .iter()
                    .find(|pair| pair.0 == code)
                    .map(|pair| pair.1.clone())
                    .unwrap()
            };
            let mut handles = std::collections::BTreeSet::new();
            let mut layer_names = std::collections::BTreeSet::new();
            for (table, record_type) in [
                ("LTYPE", "AcDbLinetypeTableRecord"),
                ("LAYER", "AcDbLayerTableRecord"),
                ("STYLE", "AcDbTextStyleTableRecord"),
                ("APPID", "AcDbRegAppTableRecord"),
            ] {
                let head = records
                    .iter()
                    .find(|r| field(r, "0") == "TABLE" && field(r, "2") == table)
                    .unwrap();
                let owner = field(head, "5");
                assert_eq!(field(head, "330"), "0");
                assert_eq!(field(head, "100"), "AcDbSymbolTable");
                assert!(handles.insert(owner.clone()));
                let rows: Vec<_> = records.iter().filter(|r| field(r, "0") == table).collect();
                assert_eq!(field(head, "70").parse::<usize>().unwrap(), rows.len());
                assert!(!rows.is_empty(), "Both tables need their default record");
                for row in &rows {
                    assert_eq!(field(row, "330"), owner);
                    assert!(row
                        .iter()
                        .any(|pair| pair.0 == "100" && pair.1 == "AcDbSymbolTableRecord"));
                    assert!(row
                        .iter()
                        .any(|pair| pair.0 == "100" && pair.1 == record_type));
                    assert!(handles.insert(field(row, "5")), "Duplicate DXF handle");
                    if table == "LAYER" {
                        layer_names.insert(field(row, "2"));
                    }
                }
                if table == "LTYPE" {
                    assert_eq!(rows.len(), if dashed { 3 } else { 1 });
                    assert!(rows.iter().any(|r| field(r, "2") == "CONTINUOUS"));
                }
            }
            assert!(handles
                .iter()
                .all(|h| usize::from_str_radix(h, 16).unwrap() < seed));
            for record in records
                .iter()
                .filter(|r| matches!(field(r, "0").as_str(), "LINE" | "TEXT" | "SOLID"))
            {
                assert!(
                    layer_names.contains(&field(record, "8")),
                    "Undeclared graphics layer"
                );
            }
        }
    }
    #[test]
    fn projection_quality_follows_saved_paper_scale_without_changing_view_intent() {
        let (doc, scene, _) = fixture(12.8);
        let assembly = AssemblyDocumentDto::default();
        for (scale, deflection) in [
            (0.01, 1.),
            (0.25, 0.04),
            (1., 0.01),
            (4., 0.0025),
            (100., 0.0001),
            (10_000., 0.0001),
        ] {
            let mut view = doc.sheets[0].views[0].clone();
            view.scale = scale;
            let saved = view.clone();
            let request = projection_request(&view, &[view.clone()], &scene, &assembly).unwrap();
            assert_eq!(request.deflection, deflection);
            assert_eq!(view, saved);
            let mut moved = view.clone();
            moved.position = [180., 120.];
            moved.name = "Same geometry elsewhere on paper".into();
            assert_eq!(
                projection_request(&moved, &[moved.clone()], &scene, &assembly).unwrap(),
                request,
                "Placement and labels must not invalidate the exact projection cache"
            );
        }
    }
    pub(super) fn fixture(
        length: f64,
    ) -> (DrawingDocumentDto, SolidSceneDto, DrawingProjectionDto) {
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
    fn linear_labels_center_and_rotate_in_both_native_formats() {
        for (mode, end, value, point, angle) in [
            ("horizontal", [12.8, 0.], "12.80 mm", [100., 78.5], 0.),
            ("vertical", [0., 12.8], "12.80 mm", [108.5, 70.], -90.),
            (
                "aligned",
                [8., 6.],
                "10.00 mm",
                [105.1, 76.8],
                -36.86989764584402,
            ),
            (
                "aligned",
                [-8., -6.],
                "10.00 mm",
                [93.1, 60.8],
                -36.86989764584402,
            ),
        ] {
            let (mut doc, scene, mut projection) = fixture(12.8);
            let annotation: DrawingAnnotationDto = serde_json::from_value({
                let mut value = serde_json::to_value(&doc.sheets[0].annotations[0]).unwrap();
                value["mode"] = json!(mode);
                value
            })
            .unwrap();
            doc.sheets[0].annotations[0] = annotation;
            projection.bounds = [
                0_f64.min(end[0]),
                0_f64.min(end[1]),
                0_f64.max(end[0]),
                0_f64.max(end[1]),
            ];
            for anchor in &mut projection.anchors {
                anchor.point = match anchor.endpoint {
                    crate::DrawingProjectionAnchorEndpoint::Start => [0., 0.],
                    crate::DrawingProjectionAnchorEndpoint::End => end,
                };
            }
            let export = |format| {
                export_sheet(
                    &doc,
                    &scene,
                    &AssemblyDocumentDto::default(),
                    &DrawingExportRequest {
                        sheet_id: 1,
                        format,
                    },
                    |_| Ok(projection.clone()),
                )
                .unwrap()
            };
            let svg = export(DrawingExportFormat::Svg);
            let label = svg
                .lines()
                .find(|line| line.ends_with(&format!(">{value}</text>")))
                .unwrap();
            assert!(label.contains("text-anchor=\"middle\""), "{mode}: {label}");
            assert!(
                label.contains(&format!("x=\"{:.5}\" y=\"{:.5}\"", point[0], point[1])),
                "{mode}: {label}"
            );
            if angle != 0. {
                assert!(
                    label.contains(&format!(
                        "rotate({angle:.5} {:.5} {:.5})",
                        point[0], point[1]
                    )),
                    "{mode}: {label}"
                );
            }
            let note = svg
                .lines()
                .find(|line| line.contains("&lt;check &amp; fit&gt;"))
                .unwrap();
            assert!(
                !note.contains("text-anchor"),
                "ordinary notes remain left aligned"
            );
            assert!(!note.contains("transform"));
            let dxf = export(DrawingExportFormat::Dxf);
            let texts: Vec<BTreeMap<&str, &str>> = dxf
                .split("0\nTEXT\n")
                .skip(1)
                .map(|entity| {
                    let lines: Vec<_> = entity.split("\n0\n").next().unwrap().lines().collect();
                    lines
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| (pair[0], pair[1]))
                        .collect()
                })
                .collect();
            let text = texts
                .iter()
                .find(|text| text.get("1") == Some(&value))
                .unwrap();
            assert_eq!(text.get("72"), Some(&"1"), "{mode}: {text:?}");
            for (code, expected) in [
                ("10", point[0]),
                ("11", point[0]),
                ("20", 210. - point[1]),
                ("21", 210. - point[1]),
            ] {
                assert!(
                    (text[code].parse::<f64>().unwrap() - expected).abs() < 1e-5,
                    "{mode}: {text:?}"
                );
            }
            let dxf_angle = text.get("50").map_or(0., |v| v.parse::<f64>().unwrap());
            assert!(
                (dxf_angle + angle).abs() < 1e-5,
                "DXF Y-up rotation: {mode}: {text:?}"
            );
            let note = texts
                .iter()
                .find(|text| text.get("1").is_some_and(|v| v.contains("<check & fit>")))
                .unwrap();
            assert!(!note.contains_key("72"));
            assert!(!note.contains_key("50"));
        }
    }
    #[test]
    fn centered_basic_dimension_frame_follows_vertical_text() {
        let mut paper = Paper {
            size: [297., 210.],
            items: Vec::new(),
        };
        dimension_label(
            &mut paper,
            [100., 70.],
            "20.00".into(),
            &DrawingDimensionPresentationDto {
                basic: true,
                ..Default::default()
            },
            &DrawingSheetStyleDto::default(),
            Some(-90.),
        );
        let Primitive::Line { points, .. } = &paper.items[0] else {
            panic!("missing basic frame")
        };

        assert_eq!(points.len(), 5);
        assert_eq!(points.first(), points.last());
        assert!((points[0][1] + points[1][1] - 140.).abs() < 1e-9);
        assert!((points[0][0] - 95.5).abs() < 1e-9);
        assert!((points[2][0] - 101.).abs() < 1e-9);
        assert!(matches!(
            &paper.items[1],
            Primitive::Text {
                point: [100., 70.],
                centered: true,
                rotation_deg: -90.,
                ..
            }
        ));
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
        assert!(text.contains(">20.00 mm</text>"));
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
        assert!(edited.contains(">25.00 mm</text>"));
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
        assert!(dxf.contains('Ø'));
        assert!(dxf.ends_with("0\nEOF\n"));
    }
    #[test]
    fn export_preserves_drawing_responsibility_and_frames_basic_dimensions() {
        let (mut document, scene, projection) = fixture(20.);
        let sheet = &mut document.sheets[0];
        sheet.title_block = DrawingTitleBlockDto {
            author: "Designer".into(),
            checked_by: "Checker".into(),
            approved_by: "Approver".into(),
            company: "Workshop & school".into(),
            ..Default::default()
        };
        sheet.revision_table_position = Some([15., 95.]);
        sheet.revisions.push(serde_json::from_value(json!({
            "id":1,"revision":"A","description":"Increase bearing clearance","date":"2026-09-10",
            "author":"Revision author","checked_by":"Revision checker","approved_by":"Revision approver",
            "change_order":"CO-7","status":"draft"
        })).unwrap());
        document.next_revision_id = 2;
        if let DrawingAnnotationDto::LinearDimension { presentation, .. } =
            &mut sheet.annotations[0]
        {
            presentation.basic = true;
        }
        let content = export_sheet(
            &document,
            &scene,
            &AssemblyDocumentDto::default(),
            &DrawingExportRequest {
                sheet_id: 1,
                format: DrawingExportFormat::Svg,
            },
            |_| Ok(projection.clone()),
        )
        .unwrap();
        for text in [
            "Designer",
            "Checker",
            "Approver",
            "Workshop &amp; school",
            "Increase bearing clearance",
            "Revision author",
            "Revision checker",
            "Revision approver",
            "CO-7",
        ] {
            assert!(content.contains(text), "Missing drawing field: {text}");
        }
        assert!(content.contains(">20.00 mm</text>"));
        assert!(!content.contains("[20.00 mm]"));
        let mut paper = Paper {
            size: [297., 210.],
            items: Vec::new(),
        };
        dimension_label(
            &mut paper,
            [100., 70.],
            "20.00".into(),
            &DrawingDimensionPresentationDto {
                basic: true,
                ..Default::default()
            },
            &document.sheets[0].style,
            None,
        );
        assert!(
            matches!(&paper.items[0], Primitive::Line { points, .. } if points.len() == 5 && points.first() == points.last())
        );
        document.sheets[0].revision_table_position = Some([15., 205.]);
        assert!(export_sheet(
            &document,
            &scene,
            &AssemblyDocumentDto::default(),
            &DrawingExportRequest {
                sheet_id: 1,
                format: DrawingExportFormat::Svg
            },
            |_| Ok(projection.clone())
        )
        .unwrap_err()
        .contains("does not fit"));
    }

    #[test]
    fn title_block_rejects_overflow_in_both_formats_before_projecting_geometry() {
        let (mut document, scene, _) = fixture(20.);
        document.sheets[0].title_block.finish = "W".repeat(4096);
        for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
            let error = export_sheet(
                &document,
                &scene,
                &AssemblyDocumentDto::default(),
                &DrawingExportRequest {
                    sheet_id: 1,
                    format,
                },
                |_| panic!("an overflowing title block must fail before geometry projection"),
            )
            .unwrap_err();
            assert!(error.contains("material / finish"));
            assert!(error.contains("minimum readable height"));
        }
    }

    #[test]
    fn title_block_preserves_actual_flagship_metadata_with_bounded_svg_and_dxf() {
        let recipes = [
            (
                include_str!("../../../examples/scripts/garden-bench.limo.jsonc"),
                22,
            ),
            (
                include_str!("../../../examples/scripts/d-screw-vise.limo.jsonc"),
                7,
            ),
            (
                include_str!("../../../examples/scripts/vertical-axis-turbine.limo.jsonc"),
                14,
            ),
            (
                include_str!("../../../examples/scripts/turbine-fit-coupons.limo.jsonc"),
                4,
            ),
        ];
        for (source, expected_sheets) in recipes {
            let json = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            let recipe: serde_json::Value = serde_json::from_str(&json).unwrap();
            let mut found = 0;
            for step in recipe["steps"].as_array().unwrap() {
                if step["call"]["operation"] != "drawing_create_sheet" {
                    continue;
                }
                found += 1;
                let arguments = &step["call"]["arguments"];
                let (mut document, scene, projection) = fixture(20.);
                let sheet = &mut document.sheets[0];
                sheet.name = arguments["name"].as_str().unwrap().into();
                sheet.title_block =
                    serde_json::from_value(arguments["title_block"].clone()).unwrap();
                if let Some(tolerance) = arguments.get("tolerance_note") {
                    sheet.tolerance_note = serde_json::from_value(tolerance.clone()).unwrap();
                }
                let mut paper = Paper {
                    size: [420., 297.],
                    items: Vec::new(),
                };
                draw_title_and_revisions(
                    &mut paper,
                    sheet,
                    limo_cad_core::UnitSystem::Mm,
                    &mut Marks::new(),
                )
                .unwrap_or_else(|error| panic!("{}: {error}", sheet.name));
                let text = paper
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Primitive::Text {
                            value,
                            height,
                            point,
                            fitted_width: Some(width),
                            ..
                        } => {
                            assert!(*height >= title_block::MIN_HEIGHT);
                            assert!(*width <= 177. + 1e-9);
                            assert!(point[0] >= 231.5 && point[0] + width <= 408.5 + 1e-9);
                            assert!(point[1] >= 244. && point[1] <= 286.);
                            Some(value.as_str())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let compact = |value: &str| {
                    value
                        .chars()
                        .filter(|ch| !ch.is_whitespace())
                        .collect::<String>()
                };
                for value in [
                    sheet.name.as_str(),
                    sheet.title_block.title.as_str(),
                    sheet.title_block.drawing_number.as_str(),
                    sheet.title_block.revision.as_str(),
                    sheet.title_block.material.as_str(),
                    sheet.title_block.finish.as_str(),
                    sheet.tolerance_note.custom.as_str(),
                ] {
                    assert!(
                        compact(&text).contains(&compact(value)),
                        "Missing field: {value}"
                    );
                }
                document.sheets[0].style.font_family = "A custom wider font".into();
                let exported = |format| {
                    export_sheet(
                        &document,
                        &scene,
                        &AssemblyDocumentDto::default(),
                        &DrawingExportRequest {
                            sheet_id: 1,
                            format,
                        },
                        |_| Ok(projection.clone()),
                    )
                    .unwrap()
                };
                let svg = exported(DrawingExportFormat::Svg);
                assert!(svg.contains("textLength="));
                assert!(svg.contains("lengthAdjust=\"spacingAndGlyphs\""));
                let dxf = exported(DrawingExportFormat::Dxf);
                assert!(dxf.contains("72\n5\n73\n0\n11\n"));
                for item in &paper.items {
                    if let Primitive::Text { value, .. } = item {
                        assert!(svg.contains(&format!(">{}</text>", xml(value))));
                        assert!(dxf.contains(&format!("\n1\n{}\n", dxf_text(value))));
                    }
                }
            }
            assert_eq!(found, expected_sheets);
        }
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

    #[test]
    fn derived_source_markers_follow_parent_topology_in_svg_and_dxf() {
        let (mut doc, scene, projection) = fixture(20.);
        let anchor = |endpoint| json!({"body_id":1,"edge_id":1,"edge_key":"bottom","endpoint":endpoint,"fallback_point":[999.,999.,999.]});
        let child: DrawingViewDto = serde_json::from_value(json!({
            "id":2,"name":"Section cut","kind":"section","direction":[0.,-1.,0.],"up":[0.,0.,1.],
            "position":[200.,140.],"scale":0.5,
            "derivation":{"type":"section","parent_view_id":1,"first":anchor("start"),"second":anchor("end"),"label":"Section A-A","hatch_angle_deg":45.,"hatch_spacing_mm":2.}
        })).unwrap();

        doc.sheets[0].views.insert(0, child);
        doc.next_view_id = 3;
        let export = |format, document: &DrawingDocumentDto, projected: &DrawingProjectionDto| {
            export_sheet(
                document,
                &scene,
                &AssemblyDocumentDto::default(),
                &DrawingExportRequest {
                    sheet_id: 1,
                    format,
                },
                |_| Ok(projected.clone()),
            )
        };
        let svg = export(DrawingExportFormat::Svg, &doc, &projection).unwrap();
        assert!(svg.contains(
            "data-layer=\"CUTTING_PLANE\" points=\"76.00000,70.00000 124.00000,70.00000\""
        ));
        assert!(svg.contains("<polygon data-layer=\"CUTTING_PLANE\" points=\"76.00000,70.00000 75.08800,72.40000 76.91200,72.40000\""));
        assert_eq!(svg.matches("text-anchor=\"middle\">A-A</text>").count(), 2);
        assert!(!svg.contains("999.00000"));
        let dxf = export(DrawingExportFormat::Dxf, &doc, &projection).unwrap();
        assert_eq!(dxf.matches("0\nSOLID\n8\nCUTTING_PLANE\n").count(), 2);
        assert!(dxf.contains(
            "10\n76.00000\n20\n140.00000\n11\n75.08800\n21\n137.60000\n12\n76.91200\n22\n137.60000"
        ));
        assert_eq!(dxf.matches("1\nA-A\n72\n1").count(), 2);
        doc.sheets[0].views[1].position = [110., 90.];
        doc.sheets[0].views[1].scale = 3.;
        let moved = export(DrawingExportFormat::Svg, &doc, &projection).unwrap();
        assert!(moved.contains(
            "data-layer=\"CUTTING_PLANE\" points=\"76.00000,90.00000 144.00000,90.00000\""
        ));
        let mut excluded = projection.clone();
        excluded.anchors.clear();
        assert!(export(DrawingExportFormat::Svg, &doc, &excluded)
            .unwrap_err()
            .contains("parent projection"));
    }

    #[test]
    fn detail_auxiliary_and_broken_sources_use_the_parent_view() {
        let (mut doc, scene, projection) = fixture(20.);
        let anchor = json!({"body_id":1,"edge_id":1,"edge_key":"bottom","endpoint":"start","fallback_point":[999.,999.,999.]});
        let definitions = [
            json!({"type":"detail","parent_view_id":1,"center":anchor,"radius":3.,"label":"D & fit"}),
            json!({"type":"auxiliary","parent_view_id":1,"reference":{"body_id":1,"edge_id":1,"edge_key":"bottom","fallback_start":[999.,999.,999.],"fallback_end":[999.,999.,999.]},"flipped":true,"label":"AUX"}),
            json!({"type":"broken","parent_view_id":1,"axis":"horizontal","first":0.25,"second":0.75,"gap_mm":5.}),
        ];
        for (i, definition) in definitions.into_iter().enumerate() {
            let mut child = doc.sheets[0].views[0].clone();
            child.id = i as u64 + 2;
            child.position = [210., 160.];
            child.scale = 0.5;
            child.derivation = Some(serde_json::from_value(definition).unwrap());
            doc.sheets[0].views.push(child);
        }
        let mut paper = Paper {
            size: [297., 210.],
            items: vec![],
        };
        draw_derived_sources(
            &mut paper,
            &doc.sheets[0],
            &BTreeMap::from([(1, projection.clone())]),
            &scene,
            &AssemblyDocumentDto::default(),
            &mut PaperGraphicsBudget::default(),
        )
        .unwrap();
        let svg = svg(&paper, "Arial");
        assert!(svg.contains("points=\"86.00000,70.00000"));
        assert!(svg.contains("D &amp; fit"));
        assert!(svg
            .contains("data-layer=\"AUXILIARY\" points=\"100.00000,70.00000 100.00000,62.00000\""));
        assert!(svg.contains("data-layer=\"BREAK\" points=\"100.00000,70.00000"));
        let ends = section_source_extent(
            [100., 70.],
            [110., 80.],
            &doc.sheets[0].views[0],
            &projection,
        )
        .unwrap();
        assert_eq!(ends, [[96., 66.], [104., 74.]]);
        assert!(section_source_extent(
            [100., 90.],
            [110., 90.],
            &doc.sheets[0].views[0],
            &projection
        )
        .is_err());
    }

    include!("drawing_export/layout_tests.rs");
}
