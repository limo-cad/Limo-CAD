//! Disposable per-view clipping and break marks in paper coordinates.
//! Source projections stay complete for associative picking and dimensions.
use super::*;
use limo_cad_sketch::DrawingBreakAxis;
use resvg::tiny_skia::{FillRule, Mask, Path, Rect};
use std::ops::Range;

pub(super) struct ViewArtwork {
    pub hatches: Range<usize>,
    pub decoration: Decoration,
}

pub(super) enum Decoration {
    None,
    Detail {
        center: [f64; 2],
        radius: f64,
    },
    Broken {
        axis: usize,
        gap: f64,
        bounds: [f64; 4],
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Clip {
    Outside,
    Inside,
    Boundary,
}

impl Decoration {
    fn clip(&self, key: RasterKey, region: RasterRegion) -> Clip {
        let Self::Detail { center, radius } = *self else {
            return Clip::Inside;
        };
        let [x, y] = region.origin_mm;
        let far = [x + region.size_mm[0], y + region.size_mm[1]];
        let near = [center[0].clamp(x, far[0]), center[1].clamp(y, far[1])];
        let factor = f64::from(key.paper_scale) * f64::from(key.render_scale);
        let margin = (f64::from(
            raster_stroke_width(0.4, key.paper_scale, key.render_scale) * key.render_scale,
        ) * 0.5
            + 2.)
            / factor;
        if (center[0] - near[0]).hypot(center[1] - near[1]) > radius + margin {
            return Clip::Outside;
        }
        let furthest = (center[0] - x)
            .abs()
            .max((center[0] - far[0]).abs())
            .hypot((center[1] - y).abs().max((center[1] - far[1]).abs()));
        if furthest + margin < radius {
            Clip::Inside
        } else {
            Clip::Boundary
        }
    }
    pub fn visible(&self, key: RasterKey, region: RasterRegion) -> bool {
        self.clip(key, region) != Clip::Outside
    }
    pub fn mask_pixels(&self, key: RasterKey, region: RasterRegion) -> u64 {
        if self.clip(key, region) == Clip::Boundary {
            u64::from(region.dimensions[0]) * u64::from(region.dimensions[1])
        } else {
            0
        }
    }
    pub fn new(view: &DrawingViewDto, projection: &DrawingProjectionDto) -> Result<Self, String> {
        if let Some((center, radius)) =
            limo_cad_occt::drawing_export::detail_clip_circle(view, projection)?
        {
            return Ok(Self::Detail { center, radius });
        }
        if let Some(DrawingViewDerivationDto::Broken { axis, gap_mm, .. }) = view.derivation {
            let [x0, y0, x1, y1] = projection.bounds;
            let width = (x1 - x0) * view.scale;
            let height = (y1 - y0) * view.scale;
            let bounds = [
                view.position[0] - width * 0.5,
                view.position[1] - height * 0.5,
                width,
                height,
            ];
            if bounds.iter().any(|v| !v.is_finite())
                || width < 0.
                || height < 0.
                || !gap_mm.is_finite()
                || gap_mm <= 0.
            {
                return Err("Broken view lies outside finite paper coordinates".into());
            }
            return Ok(Self::Broken {
                axis: if axis == DrawingBreakAxis::Horizontal {
                    0
                } else {
                    1
                },
                gap: gap_mm.max(3.),
                bounds,
            });
        }
        Ok(Self::None)
    }

    /// Include the fixed boundary paths in the sheet's existing stroke budget.
    pub fn stroke_steps(&self, key: &SourceKey) -> Result<f64, String> {
        match *self {
            Self::None => Ok(0.),
            Self::Detail { radius, .. } => {
                if !radius.is_finite() || radius <= 0. {
                    return Err("Detail view has an invalid radius".into());
                }
                Ok(128.)
            }
            Self::Broken { axis, gap, bounds } => {
                let style = &key.layout.break_line;
                if !style.width_mm.is_finite()
                    || style.width_mm <= 0.
                    || style.dash_mm.iter().any(|n| !n.is_finite() || *n <= 0.)
                {
                    return Err("Drawing break style has invalid stroke or dash lengths".into());
                }
                let interval = style.dash_mm.iter().copied().reduce(f64::min);
                let mut steps = 0.;
                for points in break_paths(axis, gap, bounds) {
                    for pair in points.windows(2) {
                        let length = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
                        steps += 1. + interval.map_or(0., |dash| (length / dash).ceil());
                    }
                }
                Ok(steps)
            }
        }
    }

    pub fn mask(&self, key: RasterKey, region: RasterRegion) -> Result<Option<Mask>, String> {
        if self.clip(key, region) != Clip::Boundary {
            return Ok(None);
        }
        let Self::Detail { center, radius } = *self else {
            return Ok(None);
        };
        let path = circle(center, radius, key, region)?;
        let mut mask = Mask::new(region.dimensions[0], region.dimensions[1])
            .ok_or("Unable to allocate detail-view clip mask")?;
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        Ok(Some(mask))
    }

    pub fn draw(
        &self,
        pixmap: &mut Pixmap,
        source: &SourceKey,
        key: RasterKey,
        region: RasterRegion,
    ) -> Result<(), String> {
        match *self {
            Self::None => Ok(()),
            Self::Detail { center, radius } => {
                if self.clip(key, region) != Clip::Boundary {
                    return Ok(());
                }
                let path = circle(center, radius, key, region)?;
                stroke(
                    pixmap,
                    &path,
                    &DrawingLineStyleDto {
                        width_mm: 0.4,
                        dash_mm: vec![],
                    },
                    key,
                )
            }
            Self::Broken { axis, gap, bounds } => {
                let [x, y, width, height] = bounds;
                let paper = if axis == 0 {
                    [x + (width - gap) * 0.5, y - 1., gap, height + 2.]
                } else {
                    [x - 1., y + (height - gap) * 0.5, width + 2., gap]
                };
                let start = point([paper[0], paper[1]], key, region)?;
                let factor = f64::from(key.paper_scale) * f64::from(key.render_scale);
                let [w, h] = [(paper[2] * factor) as f32, (paper[3] * factor) as f32];
                if !w.is_finite() || !h.is_finite() {
                    return Err("Broken-view mask lies outside finite render coordinates".into());
                }
                let rect = Rect::from_xywh(start[0], start[1], w, h)
                    .ok_or("Broken-view mask cannot render at this scale")?;
                let mut paint = Paint::default();
                paint.set_color_rgba8(255, 255, 255, 255);
                paint.anti_alias = true;
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                for points in break_paths(axis, gap, bounds) {
                    let mut path = PathBuilder::new();
                    for (index, paper) in points.into_iter().enumerate() {
                        let [x, y] = point(paper, key, region)?;
                        if index == 0 {
                            path.move_to(x, y);
                        } else {
                            path.line_to(x, y);
                        }
                    }
                    stroke(
                        pixmap,
                        &path.finish().ok_or("Cannot build view break marks")?,
                        &source.layout.break_line,
                        key,
                    )?;
                }
                Ok(())
            }
        }
    }
}

fn break_paths(axis: usize, gap: f64, bounds: [f64; 4]) -> [[[f64; 2]; 7]; 2] {
    let center = bounds[axis] + bounds[axis + 2] * 0.5;
    let start = bounds[1 - axis];
    let end = start + bounds[3 - axis];
    let middle = (start + end) * 0.5;
    [-1., 1.].map(|sign| {
        let at = center + sign * gap * 0.5;
        [
            [at, start],
            [at, middle - 4.],
            [at - 2., middle - 2.],
            [at + 2., middle],
            [at - 2., middle + 2.],
            [at, middle + 4.],
            [at, end],
        ]
        .map(|p| if axis == 0 { p } else { [p[1], p[0]] })
    })
}

fn point(p: [f64; 2], key: RasterKey, region: RasterRegion) -> Result<[f32; 2], String> {
    let factor = f64::from(key.paper_scale) * f64::from(key.render_scale);
    let result = [
        (p[0] - region.origin_mm[0]) * factor,
        (p[1] - region.origin_mm[1]) * factor,
    ]
    .map(|v| v as f32);
    if result.iter().any(|v| !v.is_finite()) {
        return Err("Derived view lies outside finite render coordinates".into());
    }
    Ok(result)
}

fn circle(
    center: [f64; 2],
    radius: f64,
    key: RasterKey,
    region: RasterRegion,
) -> Result<Path, String> {
    let [x, y] = point(center, key, region)?;
    let radius = (radius * f64::from(key.paper_scale) * f64::from(key.render_scale)) as f32;
    if !radius.is_finite() || radius <= 0. {
        return Err("Detail view radius cannot render at this scale".into());
    }
    PathBuilder::from_circle(x, y, radius).ok_or_else(|| "Cannot build detail-view boundary".into())
}

fn stroke(
    pixmap: &mut Pixmap,
    path: &Path,
    style: &DrawingLineStyleDto,
    key: RasterKey,
) -> Result<(), String> {
    let factor = f64::from(key.paper_scale) * f64::from(key.render_scale);
    let mut stroke = Stroke {
        width: raster_stroke_width(style.width_mm as f32, key.paper_scale, key.render_scale)
            * key.render_scale,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Miter,
        ..Default::default()
    };
    if !stroke.width.is_finite() || stroke.width <= 0. {
        return Err("Derived boundary width cannot render at this scale".into());
    }
    if !style.dash_mm.is_empty() {
        let mut pattern: Vec<_> = style.dash_mm.iter().map(|v| (*v * factor) as f32).collect();
        if pattern.len() % 2 != 0 {
            pattern.extend_from_within(..);
        }
        stroke.dash = Some(
            StrokeDash::new(pattern, 0.)
                .ok_or("Derived boundary dash cannot render at this scale")?,
        );
    }
    let mut paint = Paint::default();
    paint.set_color_rgba8(36, 40, 45, 255);
    paint.anti_alias = true;
    pixmap.stroke_path(path, &paint, &stroke, Transform::identity(), None);
    Ok(())
}
