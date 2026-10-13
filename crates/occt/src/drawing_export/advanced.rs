//! Remaining saved annotation families, using the same exact references and
//! paper geometry as the native sheet. All retained primitives share its budget.
use super::*;
use crate::drawing_presentation::{
    geometry::{add, arc, length, midpoint, normal, paper_point, scale, sub, unit, P},
    references::*,
    text,
};

#[derive(Clone, Copy)]
enum Ink {
    Drawing,
    Center,
}
fn layer(ink: Ink) -> &'static str {
    match ink {
        Ink::Drawing => "ANNOTATION",
        Ink::Center => "CENTER",
    }
}
struct Art<'a> {
    sink: graphics::Graphics<'a>,
    error: Option<String>,
}
impl Art<'_> {
    fn ready(&self) -> bool {
        self.error.is_none()
    }
    fn retain(&mut self, result: Result<(), String>) {
        if self.error.is_none() {
            self.error = result.err();
        }
    }
    fn polyline(&mut self, points: &[P], style: &DrawingLineStyleDto, ink: Ink) {
        if !self.ready() {
            return;
        }
        let result = self.sink.line(points, layer(ink), style);
        self.retain(result);
    }
    fn line(&mut self, a: P, b: P, style: &DrawingLineStyleDto, ink: Ink) {
        self.polyline(&[a, b], style, ink);
    }
    fn triangle(&mut self, p: [P; 3], layer: &'static str) {
        if !self.ready() {
            return;
        }
        let result = self.sink.triangle(p, layer);
        self.retain(result);
    }
    fn circle(&mut self, center: P, radius: f64, style: &DrawingLineStyleDto, ink: Ink) {
        self.polyline(&arc(center, radius, 0., std::f64::consts::TAU), style, ink);
    }
    fn disc(&mut self, center: P, radius: f64, stroke_width: f64) {
        let points = arc(
            center,
            (radius - stroke_width * 0.5).max(0.),
            0.,
            std::f64::consts::TAU,
        );
        for pair in points.windows(2) {
            self.triangle([center, pair[0], pair[1]], TEXT_MASK);
        }
    }
    fn arrow(&mut self, tip: P, toward: P, size: f64, ink: Ink) {
        if let Some(dir) = unit(sub(toward, tip)) {
            let base = add(tip, scale(dir, size));
            let side = scale(normal(dir), size * 0.3);
            self.triangle([tip, add(base, side), sub(base, side)], layer(ink));
        }
    }
    fn label(&mut self, baseline: P, value: String, size: f64, align: f64, mask: bool, _ink: Ink) {
        for (row, line) in value.lines().enumerate() {
            if !self.ready() {
                return;
            }
            let at = add(baseline, [0., row as f64 * size * 1.25]);
            if mask {
                let [l, t, r, b] = text::label_bounds(at, line, size, align);
                self.triangle([[l, t], [r, t], [r, b]], TEXT_MASK);
                self.triangle([[l, t], [r, b], [l, b]], TEXT_MASK);
            }
            let at = if align < 0. {
                sub(at, [text::width(line, size), 0.])
            } else {
                at
            };
            let result = self.sink.label(at, line, size, align == 0.);
            self.retain(result);
        }
    }
    fn leader(&mut self, attachment: P, position: P, style: &DrawingSheetStyleDto) {
        self.line(attachment, position, &style.leader, Ink::Drawing);
        self.arrow(attachment, position, style.arrow_size_mm, Ink::Drawing);
    }
    fn rect(&mut self, left: f64, top: f64, width: f64, height: f64, style: &DrawingLineStyleDto) {
        let d = style.width_mm * 0.5;
        let [l, t, r, b] = [left + d, top + d, left + width - d, top + height - d];
        self.triangle([[l, t], [r, t], [r, b]], TEXT_MASK);
        self.triangle([[l, t], [r, b], [l, b]], TEXT_MASK);
        self.polyline(
            &[
                [left, top],
                [left + width, top],
                [left + width, top + height],
                [left, top + height],
                [left, top],
            ],
            style,
            Ink::Drawing,
        );
    }
    fn center_mark(
        &mut self,
        circle: Circle,
        extension: f64,
        style: &DrawingSheetStyleDto,
    ) -> Option<()> {
        let segments =
            crate::drawing_presentation::centers::mark(circle.center, circle.radius, extension)?;
        for [a, b] in segments {
            self.line(a, b, &style.center, Ink::Center);
        }
        self.circle(
            circle.center,
            0.48,
            &DrawingLineStyleDto {
                width_mm: 0.36,
                dash_mm: vec![],
            },
            Ink::Center,
        );
        self.disc(circle.center, 0.48, 0.36);
        Some(())
    }
}

pub(super) fn supports(annotation: &DrawingAnnotationDto) -> bool {
    matches!(
        annotation,
        DrawingAnnotationDto::ChamferNote { .. }
            | DrawingAnnotationDto::CenterLineBetweenEdges { .. }
            | DrawingAnnotationDto::AutomaticSymmetryAxis { .. }
            | DrawingAnnotationDto::BoltCircleCenterLine { .. }
            | DrawingAnnotationDto::ArcLengthDimension { .. }
            | DrawingAnnotationDto::JoggedRadiusDimension { .. }
            | DrawingAnnotationDto::DatumFeature { .. }
            | DrawingAnnotationDto::GdtFrame { .. }
            | DrawingAnnotationDto::SurfaceTexture { .. }
            | DrawingAnnotationDto::EdgeRequirement { .. }
            | DrawingAnnotationDto::WeldSymbol { .. }
            | DrawingAnnotationDto::ItemBalloon { .. }
    )
}
pub(super) fn draw(
    sheet: &DrawingSheetDto,
    projections: &BTreeMap<u64, DrawingProjectionDto>,
    annotation: &DrawingAnnotationDto,
    units: limo_cad_core::UnitSystem,
    budget: &mut PaperGraphicsBudget,
) -> Result<Vec<PaperPrimitive>, String> {
    let view_id = match annotation {
        DrawingAnnotationDto::ChamferNote { view_id, .. }
        | DrawingAnnotationDto::CenterLineBetweenEdges { view_id, .. }
        | DrawingAnnotationDto::AutomaticSymmetryAxis { view_id, .. }
        | DrawingAnnotationDto::BoltCircleCenterLine { view_id, .. }
        | DrawingAnnotationDto::ArcLengthDimension { view_id, .. }
        | DrawingAnnotationDto::JoggedRadiusDimension { view_id, .. }
        | DrawingAnnotationDto::DatumFeature { view_id, .. }
        | DrawingAnnotationDto::GdtFrame { view_id, .. }
        | DrawingAnnotationDto::SurfaceTexture { view_id, .. }
        | DrawingAnnotationDto::EdgeRequirement { view_id, .. }
        | DrawingAnnotationDto::WeldSymbol { view_id, .. }
        | DrawingAnnotationDto::ItemBalloon { view_id, .. } => *view_id,
        _ => return Err("Unsupported annotation graphics route".into()),
    };
    let (view, projection) = view_projection(view_id, sheet, projections)?;
    let references = match annotation {
        DrawingAnnotationDto::BoltCircleCenterLine { features, .. } => features.len() as u64,
        _ => 4,
    };
    budget.work(
        (projection.anchors.len() as u64)
            .checked_add(projection.circles.len() as u64)
            .and_then(|n| n.checked_mul(references))
            .ok_or("Annotation resolution work overflow")?,
    )?;
    budget.scratch(64 * 1024)?;
    let mut art = Art {
        sink: graphics::Graphics::new(budget),
        error: None,
    };
    let r = Resolver { view, projection };
    if render(&mut art, sheet, annotation, r, units).is_none() {
        return Err(format!(
            "Annotation {} has a stale or incompatible projected reference",
            annotation.id()
        ));
    }
    if let Some(error) = art.error {
        return Err(error);
    }
    Ok(art.sink.finish())
}
fn render(
    art: &mut Art<'_>,
    sheet: &DrawingSheetDto,
    annotation: &DrawingAnnotationDto,
    r: Resolver<'_>,
    units: limo_cad_core::UnitSystem,
) -> Option<()> {
    use DrawingAnnotationDto::*;
    let style = &sheet.style;
    match annotation {
        ChamferNote {
            first,
            second,
            position,
            length,
            angle_deg,
            prefix,
            ..
        } => {
            art.leader(
                midpoint(r.anchor(first)?, r.anchor(second)?),
                *position,
                style,
            );
            art.label(
                add(*position, [1.2, -0.8]),
                text::chamfer(*length, *angle_deg, prefix, units, sheet.standard),
                style.text_height_mm,
                1.,
                true,
                Ink::Drawing,
            );
        }
        CenterLineBetweenEdges {
            first,
            second,
            extension,
            ..
        } => {
            let [a, b] = center_between(r.line(first)?, r.line(second)?, *extension)?;
            art.line(a, b, &style.center, Ink::Center);
        }
        AutomaticSymmetryAxis {
            axis, extension, ..
        } => {
            let b = r.projection.bounds;
            let a = paper_point(r.view, [b[0], b[1]], r.projection);
            let b = paper_point(r.view, [b[2], b[3]], r.projection);
            let c = midpoint(a, b);
            let extra = extension.max(0.);
            if *axis != DrawingOrdinateAxis::Y {
                art.line(
                    [a[0].min(b[0]) - extra, c[1]],
                    [a[0].max(b[0]) + extra, c[1]],
                    &style.center,
                    Ink::Center,
                );
            }
            if *axis != DrawingOrdinateAxis::X {
                art.line(
                    [c[0], a[1].min(b[1]) - extra],
                    [c[0], a[1].max(b[1]) + extra],
                    &style.center,
                    Ink::Center,
                );
            }
        }
        BoltCircleCenterLine {
            features,
            extension,
            ..
        } => {
            let circles: Vec<_> = features
                .iter()
                .map(|f| r.circle(f))
                .collect::<Option<_>>()?;
            if circles.len() < 3 {
                return None;
            }
            let center = scale(
                circles.iter().fold([0., 0.], |sum, c| add(sum, c.center)),
                1. / circles.len() as f64,
            );
            let radius = circles
                .iter()
                .map(|c| length(sub(c.center, center)))
                .sum::<f64>()
                / circles.len() as f64;
            if radius < 1e-5
                || circles.iter().any(|c| {
                    (length(sub(c.center, center)) - radius).abs() > 0.35_f64.max(radius * 0.015)
                })
            {
                return None;
            }
            art.circle(center, radius, &style.center, Ink::Center);
            for c in circles {
                if !art.ready() {
                    return Some(());
                }
                art.center_mark(c, *extension, style)?;
            }
        }
        ArcLengthDimension {
            feature,
            first,
            second,
            offset,
            precision,
            presentation,
            ..
        } => {
            let c = r.circle(feature)?;
            let a = sub(r.anchor(first)?, c.center);
            let b = sub(r.anchor(second)?, c.center);
            unit(a)?;
            unit(b)?;
            let start = a[1].atan2(a[0]);
            let mut sweep = (b[1].atan2(b[0]) - start).rem_euclid(std::f64::consts::TAU);
            if sweep > std::f64::consts::PI {
                sweep -= std::f64::consts::TAU;
            }
            if sweep.abs() < 1e-7 {
                return None;
            }
            let radius = c.radius + offset.max(1.);
            let points = arc(c.center, radius, start, sweep);
            art.polyline(&points, &style.dimension, Ink::Drawing);
            art.arrow(points[0], points[1], style.arrow_size_mm, Ink::Drawing);
            art.arrow(
                *points.last()?,
                points[points.len() - 2],
                style.arrow_size_mm,
                Ink::Drawing,
            );
            let angle = start + sweep * 0.5;
            let position = add(c.center, scale([angle.cos(), angle.sin()], radius + 3.));
            art.label(
                position,
                text::dimension(
                    c.model_radius * sweep.abs(),
                    *precision,
                    "⌒",
                    "",
                    units,
                    presentation,
                ),
                style.text_height_mm,
                0.,
                true,
                Ink::Drawing,
            );
        }
        JoggedRadiusDimension {
            feature,
            jog,
            position,
            precision,
            presentation,
            ..
        } => {
            let c = r.circle(feature)?;
            let dir = unit(sub(*position, c.center))?;
            let normal = normal(dir);
            let edge = add(c.center, scale(dir, c.radius));
            let before = sub(sub(*jog, scale(dir, 2.)), scale(normal, 1.5));
            let after = add(add(*jog, scale(dir, 2.)), scale(normal, 1.5));
            art.polyline(
                &[edge, before, after, *position],
                &style.dimension,
                Ink::Drawing,
            );
            art.arrow(edge, before, style.arrow_size_mm, Ink::Drawing);
            art.label(
                add(*position, [1.2, -0.8]),
                text::dimension(c.model_radius, *precision, "R", "", units, presentation),
                style.text_height_mm,
                1.,
                true,
                Ink::Drawing,
            );
        }
        DatumFeature {
            attachment,
            label,
            position,
            target_index,
            ..
        } => {
            art.leader(r.attachment(attachment)?, *position, style);
            art.rect(position[0], position[1] - 4.4, 7., 5.5, &style.leader);
            art.label(
                add(*position, [3.5, -0.5]),
                target_index.map_or(label.clone(), |n| format!("{label}{n}")),
                style.text_height_mm,
                0.,
                false,
                Ink::Drawing,
            );
        }
        GdtFrame {
            attachment,
            position,
            ..
        } => {
            art.leader(r.attachment(attachment)?, *position, style);
            let mut x = position[0];
            for cell in text::gdt_cells(annotation) {
                if !art.ready() {
                    return Some(());
                }
                let width =
                    (cell.chars().filter(|c| *c != '\u{fe0e}').count() as f64 * 2.1 + 3.).max(7.);
                art.rect(x, position[1] - 5., width, 6., &style.leader);
                art.label(
                    [x + width * 0.5, position[1] - 0.8],
                    cell,
                    style.text_height_mm,
                    0.,
                    false,
                    Ink::Drawing,
                );
                x += width;
            }
        }
        SurfaceTexture {
            attachment,
            position,
            roughness_ra,
            process,
            ..
        } => {
            art.leader(r.attachment(attachment)?, *position, style);
            art.polyline(
                &[
                    *position,
                    add(*position, [3., -6.]),
                    add(*position, [6., 0.]),
                ],
                &style.leader,
                Ink::Drawing,
            );
            art.line(
                add(*position, [3., -6.]),
                add(*position, [9., -6.]),
                &style.leader,
                Ink::Drawing,
            );
            art.label(
                add(*position, [7., -2.]),
                format!(
                    "Ra {}{}",
                    text::trim(*roughness_ra, 4),
                    if process.is_empty() {
                        String::new()
                    } else {
                        format!(" {process}")
                    }
                ),
                style.text_height_mm,
                1.,
                true,
                Ink::Drawing,
            );
        }
        EdgeRequirement {
            attachment,
            position,
            upper_deviation,
            lower_deviation,
            note,
            ..
        } => {
            let [a, b] = r.line(attachment)?;
            art.leader(midpoint(a, b), *position, style);
            art.polyline(
                &[
                    *position,
                    add(*position, [0., -5.]),
                    add(*position, [5., -5.]),
                ],
                &style.leader,
                Ink::Drawing,
            );
            let value = format!(
                "{}{} / {}{}",
                if *upper_deviation >= 0. { "+" } else { "" },
                text::trim(*upper_deviation, 4),
                text::trim(*lower_deviation, 4),
                if note.is_empty() {
                    String::new()
                } else {
                    format!(" {note}")
                }
            );
            art.label(
                add(*position, [6., -1.5]),
                value,
                style.text_height_mm,
                1.,
                true,
                Ink::Drawing,
            );
        }
        WeldSymbol {
            attachment,
            position,
            weld_type,
            size,
            length,
            pitch,
            all_around,
            field_weld,
            tail,
            ..
        } => {
            let [a, b] = r.line(attachment)?;
            art.leader(midpoint(a, b), *position, style);
            art.line(
                *position,
                add(*position, [25., 0.]),
                &style.leader,
                Ink::Drawing,
            );
            weld(art, *position, *weld_type, &style.leader);
            if *all_around {
                art.circle(*position, 1.8, &style.leader, Ink::Drawing);
                art.disc(*position, 1.8, style.leader.width_mm);
            }
            if *field_weld {
                art.polyline(
                    &[
                        add(*position, [0., -1.]),
                        add(*position, [0., -7.]),
                        add(*position, [4., -5.]),
                        add(*position, [0., -3.]),
                    ],
                    &style.leader,
                    Ink::Drawing,
                );
            }
            let value = format!(
                "{}{}{}",
                text::trim(*size, 4),
                length.map_or(String::new(), |v| format!("-{}", text::trim(v, 4))),
                pitch.map_or(String::new(), |v| format!(" ({})", text::trim(v, 4)))
            );
            art.label(
                add(*position, [8., -1.5]),
                value,
                style.text_height_mm,
                1.,
                true,
                Ink::Drawing,
            );
            if !tail.is_empty() {
                art.label(
                    add(*position, [26., -1.5]),
                    tail.clone(),
                    style.text_height_mm,
                    1.,
                    true,
                    Ink::Drawing,
                );
            }
        }
        ItemBalloon {
            attachment,
            position,
            bom_item_id,
            ..
        } => {
            art.leader(r.attachment(attachment)?, *position, style);
            let center = add(*position, [4., -2.]);
            art.circle(center, 4., &style.leader, Ink::Drawing);
            art.disc(center, 4., style.leader.width_mm);
            art.label(
                add(*position, [4., -0.8]),
                sheet
                    .bom
                    .iter()
                    .find(|item| item.id == *bom_item_id)
                    .map_or(bom_item_id.to_string(), |item| item.item_number.clone()),
                style.text_height_mm,
                0.,
                false,
                Ink::Drawing,
            );
        }
        _ => return None,
    }
    Some(())
}

fn weld(art: &mut Art, position: P, kind: DrawingWeldType, style: &DrawingLineStyleDto) {
    use DrawingWeldType::*;
    let points: &[P] = match kind {
        Fillet => &[[3., 0.], [7., 0.], [7., -4.], [3., 0.]],
        SquareGroove => &[],
        VGroove => &[[3., -4.], [6., 0.], [9., -4.]],
        BevelGroove => &[[4., -4.], [4., 0.], [8., -4.]],
        PlugSlot => &[[3., -4.], [9., -4.], [9., 0.], [3., 0.], [3., -4.]],
        Seam => &[],
        Spot | UGroove | JGroove | Surfacing => &[],
    };
    if !points.is_empty() {
        art.polyline(
            &points.iter().map(|p| add(position, *p)).collect::<Vec<_>>(),
            style,
            Ink::Drawing,
        );
    }
    match kind {
        SquareGroove => {
            for x in [4., 7.] {
                art.line(
                    add(position, [x, -4.]),
                    add(position, [x, 4.]),
                    style,
                    Ink::Drawing,
                );
            }
        }
        Seam => {
            for y in [-2., -4.] {
                art.line(
                    add(position, [3., y]),
                    add(position, [9., y]),
                    style,
                    Ink::Drawing,
                );
            }
        }
        Spot => art.circle(add(position, [6., 0.]), 2., style, Ink::Drawing),
        UGroove | JGroove | Surfacing => {
            let curve = |a: P, b: P, c: P| {
                (0..=16)
                    .map(|i| {
                        let t = i as f64 / 16.;
                        add(
                            position,
                            add(
                                add(scale(a, (1. - t) * (1. - t)), scale(b, 2. * (1. - t) * t)),
                                scale(c, t * t),
                            ),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            if kind == Surfacing {
                art.polyline(&curve([3., -1.], [6., -6.], [9., -1.]), style, Ink::Drawing);
            } else if kind == UGroove {
                art.polyline(&curve([3., -4.], [3., 0.], [6., 0.]), style, Ink::Drawing);
                art.polyline(&curve([6., 0.], [9., 0.], [9., -4.]), style, Ink::Drawing);
            } else {
                art.line(
                    add(position, [3., -4.]),
                    add(position, [3., 0.]),
                    style,
                    Ink::Drawing,
                );
                art.polyline(&curve([3., 0.], [7., 0.], [7., -4.]), style, Ink::Drawing);
            }
        }
        _ => {}
    }
}
