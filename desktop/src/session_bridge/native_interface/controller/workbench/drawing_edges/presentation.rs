//! Cached paper presentation from the same section helpers used by export.
use super::super::{Ink, LabelAlign};
use super::*;
use limo_cad_occt::drawing_export::{section_hatch_tiled, HatchPattern, PaperGraphicsLimits};
use resvg::tiny_skia::FillRule;

type PaperPresentation = Result<
    (
        Vec<PaperPrimitive>,
        Vec<derived::ViewArtwork>,
        Vec<PaperPrimitive>,
        Vec<Label>,
        usize,
    ),
    String,
>;

pub(super) fn build(
    key: &SourceKey,
    projections: &Projections,
    mut sources: impl FnMut(
        &Projections,
        &mut PaperGraphicsBudget,
    ) -> Result<Vec<PaperPrimitive>, String>,
    limits: Limits,
    projected_points: usize,
    projected_bytes: usize,
    mut stroke_steps: f64,
) -> PaperPresentation {
    let view_bytes = key
        .layout
        .views
        .len()
        .checked_mul(std::mem::size_of::<derived::ViewArtwork>())
        .ok_or("Derived view metadata exceeds the retained geometry budget")?;
    let projected_bytes = projected_bytes
        .checked_add(view_bytes)
        .filter(|bytes| *bytes <= limits.retained_bytes)
        .ok_or("Derived view metadata exceeds the retained geometry budget")?;
    let mut view_art = Vec::new();
    view_art
        .try_reserve_exact(key.layout.views.len())
        .map_err(|_| "Unable to allocate derived-view metadata")?;
    let defaults = PaperGraphicsLimits::default();
    let mut budget = PaperGraphicsBudget::new(PaperGraphicsLimits {
        points: defaults
            .points
            .min(limits.points.saturating_sub(projected_points)),
        retained_bytes: defaults
            .retained_bytes
            .min(limits.retained_bytes.saturating_sub(projected_bytes)),
        ..defaults
    });
    let mut hatches = Vec::new();
    for view_key in &key.layout.views {
        let (view, projection) = &projections[&view_key.id];
        let first = hatches.len();
        if let Some(
            DrawingViewDerivationDto::Section {
                hatch_angle_deg, ..
            }
            | DrawingViewDerivationDto::RemovedSection {
                hatch_angle_deg, ..
            },
        ) = view.derivation
        {
            let lines = section_hatch_tiled(
                view,
                projection,
                &key.layout.hatch,
                HatchPattern {
                    angle_deg: hatch_angle_deg + 90.,
                    spacing_mm: key.layout.hatch_spacing_mm,
                },
                &mut budget,
            )?;
            budget.append(&mut hatches, lines)?;
        }
        let decoration = derived::Decoration::new(view, projection)?;
        stroke_steps += decoration.stroke_steps(key)?;
        if !stroke_steps.is_finite() || stroke_steps > limits.stroke_steps {
            return Err(
                "Drawing derived boundaries exceed the stroke-step rendering budget".into(),
            );
        }
        view_art.push(derived::ViewArtwork {
            hatches: first..hatches.len(),
            decoration,
        });
    }
    let marks = if key
        .layout
        .views
        .iter()
        .any(|view| view.derivation.is_some())
    {
        sources(projections, &mut budget)?
    } else {
        vec![]
    };
    let mut label_bytes = 0usize;
    for primitive in hatches.iter().chain(&marks) {
        match primitive {
            PaperPrimitive::Line {
                points,
                width,
                dash,
                ..
            } => {
                if !width.is_finite()
                    || *width <= 0.
                    || dash.iter().any(|x| !x.is_finite() || *x <= 0.)
                    || points.iter().flatten().any(|x| !x.is_finite())
                {
                    return Err("Drawing section graphics contain invalid linework".into());
                }
                let interval = dash.iter().copied().reduce(f64::min);
                for pair in points.windows(2) {
                    let length = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
                    stroke_steps += 1. + interval.map_or(0., |dash| (length / dash).ceil());
                }
            }
            PaperPrimitive::Triangle { points, .. } => {
                if points.iter().flatten().any(|x| !x.is_finite()) {
                    return Err("Drawing section graphics contain invalid arrowheads".into());
                }
                stroke_steps += 3.;
            }
            PaperPrimitive::Text {
                point,
                value,
                height,
                rotation_deg,
                ..
            } => {
                if point.iter().any(|x| !x.is_finite())
                    || !height.is_finite()
                    || *height <= 0.
                    || !rotation_deg.is_finite()
                {
                    return Err("Drawing section graphics contain invalid labels".into());
                }
                label_bytes = label_bytes
                    .saturating_add(std::mem::size_of::<Label>())
                    .saturating_add(value.len());
            }
        }
        if !stroke_steps.is_finite() || stroke_steps > limits.stroke_steps {
            return Err("Drawing section graphics exceed the stroke-step rendering budget".into());
        }
    }
    let retained_bytes = projected_bytes
        .saturating_add(budget.usage().retained_bytes)
        .saturating_add(label_bytes);
    if retained_bytes > limits.retained_bytes {
        return Err("Drawing section graphics exceed the retained geometry budget".into());
    }
    let label_count = marks
        .iter()
        .filter(|p| matches!(p, PaperPrimitive::Text { .. }))
        .count();
    let mut labels = Vec::with_capacity(label_count);
    for primitive in &marks {
        let PaperPrimitive::Text {
            point,
            value,
            height,
            centered,
            rotation_deg,
            fitted_width,
            ..
        } = primitive
        else {
            continue;
        };
        let width = fitted_width
            .unwrap_or_else(|| value.chars().count() as f64 * height * 0.7 + 2.2)
            .max(height * 1.8);
        let label = Label {
            text: value.clone(),
            x: (point[0] + if *centered { 0. } else { width * 0.5 }) as f32,
            y: (point[1] - height * 0.4) as f32,
            angle: rotation_deg.to_radians() as f32,
            width_mm: width as f32,
            height_mm: (height * 1.18 + 1.5) as f32,
            text_height_mm: *height as f32,
            mask: true,
            ink: Ink::Derived,
            align: if *centered {
                LabelAlign::Center
            } else {
                LabelAlign::Start
            },
        };
        if [
            label.x,
            label.y,
            label.angle,
            label.width_mm,
            label.height_mm,
            label.text_height_mm,
        ]
        .iter()
        .any(|v| !v.is_finite())
        {
            return Err("Drawing section label lies outside finite render coordinates".into());
        }
        labels.push(label);
    }
    Ok((hatches, view_art, marks, labels, retained_bytes))
}

pub(super) fn draw(
    pixmap: &mut Pixmap,
    primitives: &[PaperPrimitive],
    key: RasterKey,
    region: RasterRegion,
) -> Result<(), String> {
    let factor = f64::from(key.paper_scale) * f64::from(key.render_scale);
    for primitive in primitives {
        let (points, layer) = match primitive {
            PaperPrimitive::Line { points, layer, .. } => (points.as_slice(), *layer),
            PaperPrimitive::Triangle { points, layer } => (points.as_slice(), *layer),
            PaperPrimitive::Text { .. } => continue,
        };
        if points.len() < 2 {
            continue;
        }
        let mut path = PathBuilder::new();
        for (index, point) in points.iter().enumerate() {
            let [x, y] = [
                (point[0] - region.origin_mm[0]) * factor,
                (point[1] - region.origin_mm[1]) * factor,
            ]
            .map(|x| x as f32);
            if !x.is_finite() || !y.is_finite() {
                return Err("Drawing section lies outside finite render coordinates".into());
            }
            if index == 0 {
                path.move_to(x, y);
            } else {
                path.line_to(x, y);
            }
        }
        let mut paint = Paint::default();
        if layer == "HATCH" {
            paint.set_color_rgba8(101, 113, 124, 230);
        } else {
            paint.set_color_rgba8(93, 80, 200, 255);
        }
        paint.anti_alias = true;
        match primitive {
            PaperPrimitive::Line { width, dash, .. } => {
                let mut stroke = Stroke {
                    width: raster_stroke_width(*width as f32, key.paper_scale, key.render_scale)
                        * key.render_scale,
                    line_cap: LineCap::Butt,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                if !dash.is_empty() {
                    let mut pattern: Vec<_> = dash.iter().map(|x| (*x * factor) as f32).collect();
                    if pattern.len() % 2 != 0 {
                        pattern.extend_from_within(..);
                    }
                    stroke.dash = Some(
                        StrokeDash::new(pattern, 0.)
                            .ok_or("Drawing section dash pattern cannot render at this scale")?,
                    );
                }
                let path = path.finish().ok_or("Unable to build section linework")?;
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
            PaperPrimitive::Triangle { .. } => {
                path.close();
                let path = path.finish().ok_or("Unable to build section arrowhead")?;
                pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            PaperPrimitive::Text { .. } => unreachable!(),
        }
    }
    Ok(())
}
