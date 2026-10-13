//! Center annotations retain the existing circular associations and extension.
use super::{super::drawing_paper, anchors, radial, Stamp};
use limo_cad_occt::drawing_presentation::centers;
use limo_cad_occt::drawing_presentation::geometry::{add, dot, length, scale, sub, unit};
use limo_cad_occt::DrawingProjectionDto;
use limo_cad_sketch::*;

pub(super) fn targets(
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
    direction: [f64; 3],
) -> Result<Vec<radial::Target>, String> {
    if projection.circles.len() > 16_384 {
        return Err("Too many circular center targets in this view".into());
    }
    let mut result = anchors::circles(view, projection, direction, true)?
        .into_iter()
        .map(|circle| radial::target(view, projection, circle, direction))
        .collect::<Result<Vec<_>, _>>()?;
    result.sort_by(|a, b| {
        a.center[0]
            .total_cmp(&b.center[0])
            .then(a.center[1].total_cmp(&b.center[1]))
    });
    Ok(result)
}
fn same(a: &DrawingCircularRefDto, b: &DrawingCircularRefDto) -> bool {
    a.occurrence_id == b.occurrence_id && a.body_id == b.body_id && a.edge_key == b.edge_key
}
#[derive(Default)]
pub(super) struct Placement {
    first: Option<(Stamp, u64, DrawingCircularRefDto, [f64; 2])>,
}
impl Placement {
    pub fn cancel(&mut self) {
        self.first = None;
    }
    pub fn active(&self) -> bool {
        self.first.is_some()
    }
    pub fn selected(&self, target: &radial::Target) -> bool {
        self.first.as_ref().is_some_and(|(_, view, reference, _)| {
            *view == target.view_id && same(reference, &target.reference)
        })
    }
    pub fn click(
        &mut self,
        stamp: &Stamp,
        target: &radial::Target,
        line: bool,
        document: &DrawingDocumentDto,
    ) -> Result<Option<DrawingDocumentDto>, String> {
        if self
            .first
            .as_ref()
            .is_some_and(|(saved, view, _, _)| saved != stamp || *view != target.view_id)
        {
            self.cancel();
        }
        if !target.reference.closed
            || !target.radius.is_finite()
            || target.radius <= 0.
            || target.center.iter().any(|n| !n.is_finite())
        {
            return Err("Choose a complete projected circle".into());
        }
        let id = document.next_annotation_id;
        let annotation = if line {
            if let Some((_, _, first, center)) = &self.first {
                if same(first, &target.reference) {
                    return Ok(None);
                }
                if length(sub(*center, target.center)) < 1e-7 {
                    return Err("Choose two distinct circular centers".into());
                }
                DrawingAnnotationDto::CenterLine {
                    id,
                    view_id: target.view_id,
                    first: first.clone(),
                    second: target.reference.clone(),
                    extension: 2.5,
                }
            } else {
                self.first = Some((
                    stamp.clone(),
                    target.view_id,
                    target.reference.clone(),
                    target.center,
                ));
                return Ok(None);
            }
        } else {
            DrawingAnnotationDto::CenterMark {
                id,
                view_id: target.view_id,
                feature: target.reference.clone(),
                extension: 2.5,
            }
        };
        let mut next = document.clone();
        next.next_annotation_id = id.checked_add(1).ok_or("Annotation IDs are exhausted")?;
        let sheet = next
            .sheets
            .iter_mut()
            .find(|s| s.id == stamp.sheet_id)
            .ok_or("Drawing sheet changed")?;
        sheet.annotations.push(annotation);
        if sheet.release.status == DrawingReleaseStatus::Released {
            sheet.release.status = DrawingReleaseStatus::Draft;
        }
        next.validate()?;
        self.cancel();
        Ok(Some(next))
    }
}
#[derive(Clone, Copy)]
pub(super) struct Grip {
    pub origin: [f64; 2],
    pub direction: [f64; 2],
    pub point: [f64; 2],
}
#[derive(Clone)]
pub(super) struct Geometry {
    pub segments: Vec<[[f64; 2]; 2]>,
    pub grips: Vec<Grip>,
}
pub(super) fn geometry(
    annotation: &DrawingAnnotationDto,
    view: &DrawingViewDto,
    projection: &DrawingProjectionDto,
) -> Option<Geometry> {
    let circle = |r| {
        drawing_paper::resolved_center_circle(view, projection, r)
            .filter(|(c, r)| c.iter().all(|n| n.is_finite()) && r.is_finite() && *r > 0.)
    };
    let (extension, origins) = match annotation {
        DrawingAnnotationDto::CenterLineBetweenEdges {
            first,
            second,
            extension,
            ..
        } => {
            let r = limo_cad_occt::drawing_presentation::references::Resolver { view, projection };
            let [start, end] = limo_cad_occt::drawing_presentation::references::center_between(
                r.line(first)?,
                r.line(second)?,
                0.,
            )?;
            let direction = unit(sub(end, start))?;
            (
                *extension,
                vec![(start, scale(direction, -1.)), (end, direction)],
            )
        }
        DrawingAnnotationDto::AutomaticSymmetryAxis {
            axis, extension, ..
        } => {
            let b = projection.bounds;
            let a = drawing_paper::paper_point(view, [b[0], b[1]], projection);
            let b = drawing_paper::paper_point(view, [b[2], b[3]], projection);
            let c = scale(add(a, b), 0.5);
            let mut origins = Vec::new();
            if *axis != DrawingOrdinateAxis::Y {
                origins.extend([
                    ([a[0].min(b[0]), c[1]], [-1., 0.]),
                    ([a[0].max(b[0]), c[1]], [1., 0.]),
                ]);
            }
            if *axis != DrawingOrdinateAxis::X {
                origins.extend([
                    ([c[0], a[1].min(b[1])], [0., -1.]),
                    ([c[0], a[1].max(b[1])], [0., 1.]),
                ]);
            }
            (*extension, origins)
        }
        DrawingAnnotationDto::BoltCircleCenterLine {
            features,
            extension,
            ..
        } => {
            let circles: Vec<_> = features.iter().map(circle).collect::<Option<_>>()?;
            if circles.len() < 3 || !extension.is_finite() || !(0. ..=1e6).contains(extension) {
                return None;
            }
            let center = scale(
                circles.iter().fold([0., 0.], |sum, (c, _)| add(sum, *c)),
                1. / circles.len() as f64,
            );
            let radius = circles
                .iter()
                .map(|(c, _)| length(sub(*c, center)))
                .sum::<f64>()
                / circles.len() as f64;
            if radius < 1e-5
                || circles.iter().any(|(c, _)| {
                    (length(sub(*c, center)) - radius).abs() > 0.35_f64.max(radius * 0.015)
                })
            {
                return None;
            }
            let mut segments: Vec<_> = (0..64)
                .map(|i| {
                    std::array::from_fn(|j| {
                        let angle = (i + j) as f64 * std::f64::consts::TAU / 64.;
                        add(center, [radius * angle.cos(), radius * angle.sin()])
                    })
                })
                .collect();
            for (c, r) in &circles {
                segments.extend(centers::mark(*c, *r, *extension)?);
            }
            let (c, r) = circles[0];
            let origin = add(c, [r, 0.]);
            return Some(Geometry {
                segments,
                grips: vec![Grip {
                    origin,
                    direction: [1., 0.],
                    point: add(origin, [*extension, 0.]),
                }],
            });
        }
        DrawingAnnotationDto::CenterMark {
            view_id,
            feature,
            extension,
            ..
        } if *view_id == view.id => {
            let (c, r) = circle(feature)?;
            let [horizontal, vertical] = centers::mark(c, r, 0.)?;
            (
                *extension,
                vec![
                    (horizontal[0], [-1., 0.]),
                    (horizontal[1], [1., 0.]),
                    (vertical[0], [0., -1.]),
                    (vertical[1], [0., 1.]),
                ],
            )
        }
        DrawingAnnotationDto::CenterLine {
            view_id,
            first,
            second,
            extension,
            ..
        } if *view_id == view.id => {
            let (a, ra) = circle(first)?;
            let (b, rb) = circle(second)?;
            let d = unit(sub(b, a))?;
            let [start, end] = centers::line(a, ra, b, rb, 0.)?;
            (*extension, vec![(start, scale(d, -1.)), (end, d)])
        }
        _ => return None,
    };
    if !extension.is_finite() || !(0. ..=1e6).contains(&extension) {
        return None;
    }
    let grips: Vec<_> = origins
        .into_iter()
        .map(|(origin, direction)| Grip {
            origin,
            direction,
            point: add(origin, scale(direction, extension)),
        })
        .collect();
    if grips
        .iter()
        .any(|grip| grip.point.iter().any(|n| !n.is_finite()))
    {
        return None;
    }
    let segments = grips
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| [pair[0].point, pair[1].point])
        .collect();
    Some(Geometry { segments, grips })
}
pub(super) fn extension_at(grip: Grip, point: [f64; 2]) -> Result<f64, String> {
    let extension = dot(sub(point, grip.origin), grip.direction).max(0.);
    if point.iter().any(|n| !n.is_finite()) || !extension.is_finite() || extension > 1e6 {
        return Err("Center extension must be at most 1000000 paper mm".into());
    }
    Ok(extension)
}
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod pick_tests;
#[cfg(test)]
mod tests;
